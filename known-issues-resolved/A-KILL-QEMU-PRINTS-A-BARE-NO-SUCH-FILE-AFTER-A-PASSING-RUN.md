### A-KILL-QEMU-PRINTS-A-BARE-NO-SUCH-FILE-AFTER-A-PASSING-RUN. The `[ -f ]` guard on the pidfile does not make the read that follows it safe, and the read's own `2>/dev/null` cannot suppress the message — 2026-09-02 — **Status: ✅ FIXED 2026-09-02** (lane A; the fix below was applied verbatim once the in-flight boot test released `scripts/`. `bash -n` and `shellcheck --severity=warning` both clean.)

**Fixed as described.** The `[ -f ]` wrapper is gone, the read is attempted
unconditionally, and the stderr redirect moved to the group so it covers the
shell's own diagnostic. One thing was added beyond the plan: the stale docblock
claimed a kill-by-image-name fallback that no longer exists, and rather than
just deleting the sentence the replacement records *why* there must never be
one again — with three lanes live, killing `qemu-system-x86_64.exe` by image
name would take down another lane's boot test along with ours. A removed
fallback that is only deleted invites its own reintroduction.

**In short:** after a *successful* boot test, the harness sometimes prints one
stray line — `./scripts/boot-test.sh: line 1329: .../build/qemu.pid: No such
file or directory` — and then exits 0 anyway. Nothing is broken; the run is
genuinely green. But an unexplained error printed on a passing run is exactly
the thing that makes a reader distrust the next one, and it costs someone a
diff-read to establish that it means nothing.

**Where it lives.** `scripts/boot-test.sh:1327-1329`, in `kill_qemu()`:

```bash
    if [ -f "$PIDFILE" ]; then
        local win_pid
        win_pid="$(tr -cd '0-9' < "$PIDFILE" 2>/dev/null || true)"
```

**Two independent defects, and the second is the interesting one.**

1. **A TOCTOU race.** QEMU unlinks its own `-pidfile` as it exits. On the happy
   path the guest reaches BOOT_OK and QEMU is on its way out under its own
   power, so the file can — and sometimes does — vanish between the `[ -f ]` on
   1327 and the redirection on 1329. That the message shows up *after a passing
   run* rather than a failing one is not a coincidence; it is the signature.

2. **`2>/dev/null` is attached to the wrong thing, and cannot be attached to
   the right one from where it sits.** A failed *input redirection* is
   diagnosed by the shell, which never execs `tr` at all — so `tr`'s stderr
   redirection is never in effect when the message is produced, and the message
   goes to the script's stderr. `|| true` then swallows the *status*, which is
   why the run stays green and the line appears alone with nothing else wrong.
   Verified directly:

   ```
   $ bash -c 'v="$(tr -cd "0-9" < /tmp/missing 2>/dev/null || true)"; echo "[$v]"'
   bash: line 1: /tmp/missing: No such file or directory
   []
   $ bash -c 'v="$( { tr -cd "0-9" < /tmp/missing; } 2>/dev/null || true)"; echo "[$v]"'
   []
   ```

**The proper fix — remove the race, do not narrow it.** Delete the `[ -f ]`
wrapper and let the read simply be attempted, with the *group's* stderr
redirected so it covers the shell's own diagnostic as well as `tr`'s:

```bash
    # QEMU unlinks its own pidfile as it exits, so a test-then-read races the
    # very teardown this function exists to perform, and loses most often on
    # the happy path. Attempt the read unconditionally instead: an absent file
    # yields an empty pid, which the `-n` below already handles. The stderr
    # redirect is on the *group* because a failed input redirection is reported
    # by the shell before `tr` is ever exec'd, so `tr`'s own 2>/dev/null never
    # covers it -- which is how this printed a bare "No such file or directory"
    # on runs that passed.
    local win_pid
    win_pid="$( { tr -cd '0-9' < "$PIDFILE"; } 2>/dev/null || true)"
    if [ -n "$win_pid" ]; then
        taskkill //F //PID "$win_pid" >/dev/null 2>&1 || true
    fi
```

Dropping the `[ -f ]` is part of the fix and not a simplification of it. Left
in place it reads to the next person as though it establishes that the file is
there for the line beneath it, which is the property it does not have and
cannot have. That is the same defect as `A-GATES-SILENTLY-STOPPED-CHECKING` and
as lane B's `check_shellcheck` observation — *a guard that reports a fact it has
not checked costs more than the finding does* — arriving from a third
direction, this time as a guard rather than a report.

**Why the fallback comment above it is also now wrong.** `kill_qemu`'s docblock
at :1311-1312 says it "falls back to killing by image name only if the pidfile
is missing (should not happen)." Both halves are stale: there is no
kill-by-image-name fallback in the body any more, and the pidfile going missing
demonstrably *does* happen and is in fact the normal case on a clean exit. The
comment should go with the fix.
