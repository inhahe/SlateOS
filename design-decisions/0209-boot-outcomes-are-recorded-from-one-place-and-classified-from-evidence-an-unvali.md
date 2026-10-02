## §209 — Boot outcomes are recorded from one place and classified from evidence; an unvalidated fingerprint reports as unvalidated, never as a streak

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)
**Lane:** A
**Where:** `scripts/boot-history.py`, `scripts/test-boot-history.py`,
`scripts/boot-test.sh` (EXIT trap), `scripts/wedge-soak.sh` (`BOOT_LABEL`),
`bench/boot-history.jsonl`

**In short:** four of lane A's open kernel bugs are intermittent hangs whose
closure condition is "N clean boots in a row" — and until now nothing counted
the boots. W1's status line has said "clean streak **7**" since 2026-06-14
while many dozens of boots have passed; the entry itself calls that "stale
bookkeeping, not a real count." The boot test now writes one row per run to
`bench/boot-history.jsonl`, and the streaks become a query instead of a number
someone has to remember to edit. Two things were decided on the way, each with
a real alternative: **where** the recorder is called from, and **what a
fingerprint that has never matched is allowed to print**.

### The problem the numbers were solving

`bench/history.jsonl` already exists and is the precedent — `bench-history.py`
opens with the same argument in its own voice: *"boot-test.sh already tells the
reader to compare against prior runs — but nothing stored the prior runs, so
that advice was unfollowable."* It stores numbers. It cannot store this: it only
gains a record on a `--bench` run that reached its marker, so it is
structurally blind to precisely the runs that matter here — the ones that never
got there.

Meanwhile the evidence for a failed boot lives in `build/serial-test.txt`, which
is gitignored per-worktree scratch and is overwritten by the next run. So a hang
is preserved only if a human pastes it into markdown before the next boot. That
loss already cost an investigation once; `boot-test.sh`'s own comment says so at
the tail-printing block: *"this exact loss bit the fork+exec-hang investigation."*

### Decision 1 — one call site in the EXIT trap, verdict derived from `(exit code, serial log)`

| Option | Why not |
|---|---|
| Call the recorder at each `exit` site | `boot-test.sh` has ~12. The thirteenth one someone adds is a **failure** site, and the omission reads downstream as a clean streak — the check silently disarms in the one direction that closes bugs on no evidence. |
| Record from the callers (`wedge-soak.sh`, ad-hoc runs) | Only soaks would ever be recorded, and routine boots are the majority of the streak. The closure bars explicitly count routine boots. |
| **One call in the EXIT trap, classifying from evidence** ✓ | Cannot be forgotten by construction: every exit path goes through it. |

That forces the verdict to be *derived* rather than passed in, which turned out
to be the better design anyway: exit 1 is reached from five distinct conditions
and only the serial log distinguishes them, while the serial log alone cannot
tell a stall (exit 2) from a plain timeout, since both end with no marker. Each
source answers the half the other cannot.

Two smaller calls fell out of it, both with a failure mode worth naming:

- **`$?` is captured as the handler's first argument, before anything else
  runs.** Reaping QEMU and running the recorder both clobber it otherwise.
- **INT/TERM get a separate handler from EXIT, and record nothing.** An
  operator's Ctrl-C is not a boot outcome; recording it would reset every hang
  streak every time someone stopped a run early. A once-guard keeps the cleanup
  single, since bash runs the signal handler *and then* the EXIT handler.
- **A missing serial log is `NO_BOOT` and is not recorded at all.** It means the
  build failed or the harness died before QEMU wrote anything. Compile errors
  are far more common than hangs, so recording them as outcomes would reset
  every streak faster than it could grow.

### Decision 2 — `validated_by`, and the refusal to print a streak without it

This is the one with a genuine cost. Each fingerprint (a predicate identifying a
known issue in a failed boot's serial log) carries the list of real occurrences
it is known to match. A fingerprint with an empty list prints a warning **in
place of** its streak rather than alongside it.

*Against:* a suppressed number is a number the reader wanted. Someone adding a
fingerprint for a new bug gets no streak from it until they can point at an
occurrence — which, for a bug seen once before the recorder existed, means
reconstructing the sample from `known-issues.md`.

*For, and decisive:* a matcher that can never fire produces a **perfect** clean
streak, and a perfect clean streak is exactly what closes an issue. The number
and the bug are then indistinguishable from the outside. This is the same rule
`stamp-ancestry.py` adopted when a declared source path does not exist (§208) —
*could not verify* must never render as *fine* — and the same lesson
`test-bench-history.py`'s docstring records as the thing "the project keeps
rediscovering."

The validation is enforced by tests, not by the field: `test-boot-history.py`
reconstructs a serial sample from the evidence quoted in `known-issues.md` for
every occurrence named in `validated_by`, asserts each matches its own
fingerprint **and no other**, and fails if a fingerprint claims validation
without a sample. A comment claiming "this matches the 2026-06-12 hang" cannot
fire either; a test can.

### Two fingerprints that shaped the rest

- **W1 is fingerprinted on the *silence*, not on the OOM self-test.** Its
  2026-08-14 analysis argues the console-lock fixes mean a re-entry now *prints*
  instead of spinning, and recommends retargeting the bar "from ~90 blind boots
  to one observation." So the predicate is: no marker, log cut **mid-line**, no
  exception and no panic anywhere. A match falsifies that analysis, which the
  entry itself says is worth more than the 83 remaining clean boots.
- **`B-PTHREAD-TEARDOWN-PF` is matched on `(address, task name)`, never on the
  RIP.** The RIP moves with every kernel rebuild, so a RIP-keyed fingerprint
  stops matching after any recompilation — and a streak that resets to *clean*
  on recompilation is worse than no streak, because it looks like progress.

### What this commits us to

Every boot from `boot-test.sh` appends a row to a committed file, and failures
carry a bounded serial tail (40 lines × 300 chars) so the freeze context
survives the next run. Passes carry none — a passing tail is the same lines
every time, and this file is appended to from three worktrees and merged as
text. The known-issues streak numbers should now be *quoted from*
`--streaks` rather than maintained by hand; the entries keep their prose, but
the counts stop being something a session has to remember.
