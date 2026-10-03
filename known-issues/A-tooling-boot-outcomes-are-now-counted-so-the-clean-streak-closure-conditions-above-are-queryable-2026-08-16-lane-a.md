## TOOLING — boot outcomes are now counted, so the "clean streak" closure conditions above are queryable (2026-08-16, lane A)

**Status: LANDED 2026-08-16** — `scripts/boot-history.py`,
`scripts/test-boot-history.py`, wired into `scripts/boot-test.sh`'s EXIT trap.

Several entries in this file close on a **count** — "a fresh combined
dedicated-soak + routine-boot clean streak past ~90 with no recurrence" (W1),
and similar bars on the other intermittent hangs. Nothing counted them. W1's
own status line has read **clean streak 7** since 2026-06-14 while many dozens
of boots have passed, and the entry says so itself: *"the recorded streak of 7
is stale bookkeeping, not a real count."*

That is not carelessness. Keeping the number right by hand means editing this
file after every boot, which nobody will do and nobody did.

**What now happens.** Every run of `scripts/boot-test.sh` appends one row to
`bench/boot-history.jsonl` — verdict, commit, branch, host, wall time, label,
and for a **failure**, the last 40 serial lines. The last part matters
independently of the counting: `build/serial-test.txt` is gitignored scratch
that the next run overwrites, so until now the evidence for a hang survived
only if somebody pasted it in here before the next boot. That loss already cost
one investigation (`B-FORKEXEC-BOOT-HANG`; `boot-test.sh`'s own comment says
so). Failures now carry their freeze context into a committed file.

**How to read it:**

```
python scripts/boot-history.py --streaks     # per-issue standing
python scripts/boot-history.py --list        # recent runs, one line each
```

**Fingerprints currently recognised**, each validated by a serial sample
reconstructed in `scripts/test-boot-history.py` from the evidence quoted in
this file:

| id | matched on |
|---|---|
| `W1` | no marker, log cut **mid-line**, no exception and no panic anywhere |
| `B-KASAN-…-WEDGES-MID-PRINT-ON-A-PAGE-FAULT` | the cut lands inside the `EXCEPTION:` line itself |
| `B-PTHREAD-TEARDOWN-PF` | `#PF` at a small fixed address with `cloned-thread` in the report |
| `B-FORKEXEC-BOOT-HANG` | quiet stop **between** lines right after the last thread is reaped |
| `W-KERNEL-COW-WRITE` | `error=0x3` write fault against a user-half address |

**Two properties that are the point, not decoration.**

1. **The verdict is derived from `(exit code, serial log)` at one call site**,
   not passed in at each of `boot-test.sh`'s ~12 `exit` sites. A recorder wired
   per-site is wrong the first time someone adds a thirteenth — and wrong in the
   direction that matters, because the site nobody wired up is a *failure* site,
   so the omission reads downstream as a clean streak.
2. **A fingerprint that has never been validated against a real occurrence
   prints a warning in place of its streak, not a number.** A matcher that
   cannot fire produces a *perfect* clean streak, and a perfect clean streak is
   exactly what closes an entry in this file. Same rule as
   `scripts/stamp-ancestry.py` (design-decisions.md §208): *could not verify*
   must never render as *fine*.

**What this does not do.** It does not retroactively count the boots that
happened before it existed, and it says so: an entry whose known occurrences
predate the file reports "the count starts at the recorder, not at the bug."
So the streaks above start at 0 today and are honest rather than flattering.
Ctrl-C and build failures are deliberately **not** recorded — an interruption
is not a boot outcome, and compile errors are common enough that recording them
would reset every streak faster than it could grow.

Rationale and the alternatives considered: design-decisions.md §209.

**Amended 2026-08-19: deliberate probes are excluded, and were not before.** A
gap in the above that only showed up when it fired. A run under a deliberately
non-default configuration — `QEMU_EXTRA`, an overridden `QEMU_CPU`, or a
`BENCH_EXPERIMENT` arm — was recorded as an ordinary boot of the tree, because
`boot-history.jsonl` had no `experiment` field at all (`bench/history.jsonl` had
carried one since the layout sweeps). On 2026-08-19 the `-cpu host` probe of
`ENV-WHPX-CPU-HOST-FIRMWARE-GP` died inside OVMF *before our kernel was loaded*,
landed as a plain `TIMEOUT`, and reset the streak to **0** — a fact about which
CPU models WHPX accepts, silently retargeting four entries' closure bars in this
file.

The second half is worse, and is why this is amended here rather than merely
fixed in code. `--streaks` counts a differently-failing boot toward a
fingerprint's `since_last`, on the argument *"a boot that failed differently is
still a boot in which this did not appear."* That argument assumes the kernel
**ran**. A probe need not have. Counting one is a manufactured clean streak
arriving through the one door with no guard on it — property 2 above, defeated
from the other side.

Fixed in `6cb8d893e`: `boot-test.sh` now records *why* a run was
non-representative, and the clean streak, the fingerprint streaks and the
wall-time medians all step over such rows. Probes still appear in `--list` —
they are excluded from *inference*, not hidden — and the report prints how many
were set aside, so a shrunken denominator is never silent. The three rows that
predated the field were backfilled in `554b2e3d0`; the three-condition test for
when editing an append-only log is legitimate at all is design-decisions.md §238.
