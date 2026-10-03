## B-TIMED-OUT-BOOT-TEST-STRANDS-THE-CROSS-WORKTREE-LOCK-FOR-20-MINUTES

**Status: FIXED 2026-08-16** by lane A, in `scripts/boot-test.sh`, with a
regression test in `scripts/test-boot-lock.sh`. See "Fix as landed" at the
end of this entry — it covers two *more* bugs the new test found in the same
acquire loop, one of which was the opposite failure (breaking a lock that had
just been legitimately taken). Reported by lane B as
`requests/b-a-boot-lock-survives-its-dead-owner.md`.

**In short:** every lane runs the boot test through a shared "only one QEMU
at a time" lock. If a boot test is killed rather than allowed to finish —
which is exactly what our own timeout runner does when a run overruns — the
lock is left behind with nobody holding it, and the *next* boot test on any
lane sits and waits for it. It eventually gives up and continues after 20
minutes, so nothing breaks permanently; the cost is that a 20-minute stall
looks indistinguishable from a hung boot, and a lane that is trying to
verify a fix loses that time for no reason. Observed today, twice in one
session.

**What actually happens.** `scripts/boot-test.sh` (~1295-1350) serialises
QEMU across the three worktrees with a `mkdir`-based lock directory in the
git *common* dir (`$(git rev-parse --git-common-dir)/slateos-boot-lock`,
i.e. `os/.git/slateos-boot-lock` — shared by all worktrees, which is why it
is not under any lane's `build/`). `release_boot_lock` removes it only when
`$BOOT_LOCK_DIR/owner` still names this process, which is the right rule —
it is what stops us deleting a lock some other lane acquired after ours was
broken. But release is reached only if the script gets to run its exit path.

`scripts/run-timeout.py`, per `CLAUDE.md`, deliberately puts the child in a
Windows Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so a timeout
tears down the **entire process tree at once** — bash included. That is the
correct behaviour for its actual job (no orphaned QEMU, ever), and it is why
we use it. The side effect is that `boot-test.sh` never executes *any* exit
path, so the lock directory outlives the process that made it. The only
thing that then clears it is the age-based breaker at line 1336, which
requires the lock to be **>1200s (20 min)** old — a threshold chosen against
our longest healthy boot (~8 min), which is correct for its own purpose and
far too slow here.

**How it presented.** A boot test on `main` timed out at 1800s (the cold
kernel build alone took 24m16s, leaving too little for the ~344s boot). The
warm re-run then logged, at 60-second intervals:

```
=== Waiting for boot lock, held by lane-B/pid-1050807/1786884746 (0s) ===
=== Waiting for boot lock, held by lane-B/pid-1050807/1786884746 (60s) ===
...                                                              (420s) ===
```

`pid-1050807` was the *timed-out* run. Nothing was running: `tasklist` showed
zero QEMU processes. Clearing the directory by hand let the waiter acquire
within one 5s poll.

**The diagnosis was made harder by a wrong first guess, which is worth
recording.** On seeing the timeout I checked `os/build/` for a lock, found
nothing, and concluded "clean teardown — no stale lock". `build/.boot-lock`
is only the *fallback* path used when `git rev-parse --git-common-dir`
fails; the real lock is in the git common dir precisely so that all four
worktrees share one. Looking for a cross-worktree lock inside one
worktree's `build/` will always find nothing and always look reassuring.

**What the fix should be: make the lock defend itself against a dead
owner, rather than only against an old one.** The owner file already records
the pid (`lane-B/pid-$$/<epoch>`), and every lane runs the script under the
same MSYS bash, so a waiter can ask whether that pid still exists:

```sh
# before consulting _lock_age, ask whether the holder is alive at all
_lock_pid="$(sed -n 's#.*/pid-\([0-9]\+\)/.*#\1#p' "$BOOT_LOCK_DIR/owner" 2>/dev/null || echo "")"
if [ -n "$_lock_pid" ] && ! kill -0 "$_lock_pid" 2>/dev/null; then
    echo "=== Breaking boot lock: owner pid $_lock_pid is gone ==="
    rm -rf "$BOOT_LOCK_DIR" 2>/dev/null || true
    continue
fi
```

Two properties matter and should be kept. It must stay **conservative in the
unknown case** — if the owner string does not parse, or `kill -0` cannot
answer, fall through to the existing 1200s age rule rather than breaking;
a lock broken while QEMU is live costs a corrupted, mutually-slowed pair of
boots, which is far worse than waiting. And it must not replace the age
breaker, which still covers the cases a pid check cannot see (a pid recycled
onto a new process, an owner from a previous Windows session, an owner in a
different MSYS instance whose pid namespace is not ours).

**Why not fix it in `run-timeout.py` instead.** It is tempting to have the
timeout runner clean up, but it must not: `run-timeout.py` is generic — it
knows nothing about boot locks and should not, and giving it lock-specific
knowledge would mean every future resource guarded this way needs another
special case in it. More decisively, it cannot be relied on for this even in
principle, because the same stranding happens when the runner *itself* dies
(power loss, harness restart, Ctrl-C at the wrong moment). The lock has to
be robust against its holder vanishing however that happens; that logic
belongs with the lock.

**Standing lesson.** A cleanup that lives only on the success path is not
cleanup — it is a comment about what usually happens. Anything holding a
shared resource must be recoverable *by the next acquirer* without help
from the process that died, because the whole class of failures worth
defending against is precisely the ones where the holder does not get to run
another line of code. Note that both halves here were individually correct:
the Job Object kill is right, and the owner-matched release is right. The
gap is only visible where they meet.

### Fix as landed (2026-08-16, lane A)

The reported bug is fixed as lane B suggested, and writing a test for it
turned up two more in the same twenty lines. All three are in the acquire
loop in `scripts/boot-test.sh`; the test is `scripts/test-boot-lock.sh`
(24 assertions, ~1s, no build required).

**1. The reported bug — a dead owner is now broken in ~60s, not 1200s.**
The owner string's pid is parsed and probed with `kill -0`. Confirmed
against the exact pid from the report above (`1050807`): dead, so the new
code would have broken that lock on its first poll.

**2. A lock with no owner file yet was broken *immediately* — the opposite
failure, and the more dangerous one.** `mkdir` acquires, and the owner file
is written on the next line. A waiter polling inside that window saw a lock
directory with no `owner` in it, and the old code scored that as
`_lock_age=999999`, which is `> 1200`, so the age breaker deleted a lock
another lane had taken microseconds earlier — putting two QEMUs on one host.
Fixed by falling back to the lock *directory's* mtime, which `mkdir` stamps
at acquisition, and treating "cannot stat either" as the only unknown case.
This had nothing to do with lane B's report; it was found by asking what the
age rule does when the owner file is absent, which is a question the test
had to answer to construct its cases.

**3. The age rule no longer overrides proven liveness.** `_lock_age > 1200`
used to break the lock unconditionally. That threshold was chosen when
liveness was *unknowable*, so age was the only available proxy for death.
Now that we can ask, a lock held by a demonstrably-live pid is never broken
at any age: a boot that outruns the 20-minute estimate is slow (cold host,
QEMU stalled on I/O), not dead, and breaking its lock produces exactly the
"two mutually-slowed, possibly corrupted boots" that lane B's request warned
against — with the added cost that both runs can then fail for reasons
unrelated to the code under test. Waiting on a live owner stays bounded by
`BOOT_LOCK_WAIT` (3600s default), which proceeds anyway rather than failing.

**Kept, as lane B asked:** liveness is a **tri-state** — alive / dead /
unknown — and unknown falls through to the age rule rather than breaking.
Unknown covers an unparseable owner string, a `kill` that cannot answer
(probed once via `kill -0 $$` on ourselves, so a broken `kill` disables the
liveness breaker instead of making every owner look dead), and a lock
younger than 60s, where a transient is likelier than a real death. The
1200s age breaker is kept in full for the cases a pid check cannot see:
a recycled pid, an owner from a previous Windows session, an owner in a
different MSYS instance's pid namespace.

**On the test.** The acquire loop runs *after* the kernel build, so reaching
it through a normal `boot-test.sh` run costs ~7 minutes per case, and the
interesting cases need a second lane holding the lock at a controlled age
with a controlled pid — which is why this logic had no test and had
accumulated three bugs. `scripts/test-boot-lock.sh` extracts the region
between the `BOOT-LOCK-REGION` markers **verbatim** and executes it, rather
than restating the logic (a restatement drifts from the original and then
tests nothing), and fails loudly if the markers go missing. Each of the
three fixes was mutation-tested — reverted in a scratch copy, confirmed the
suite goes red, restored — because a lock test that cannot fail is worth
less than no test at all.
