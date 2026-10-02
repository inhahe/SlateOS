## §669 — a host failure gets its own verdict, not a widening of `experiment`

**Date:** 2026-09-01
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** on 2026-09-01 the Windows machine running the emulator briefly
could not hand it the memory it asked for. The emulated graphics card then
refused an allocation, the kernel correctly reported that as an I/O error, a
self-test correctly called it fatal — and the boot history wrote the run down
as "the kernel died", which reset the count of consecutive successful boots
from nine to zero. Nothing in our code was wrong. The fix is a new outcome
label, `HOST_FAIL`, set only when the emulator's *own* error output says the
machine underneath ran out of resources; such a run is recorded in full but
left out of every statistic that claims to describe the health of our code.

### The problem is that a run has two ways of not being about the tree

`boot-history.py` already had one of them. A boot run under conditions no
checkout reproduces — foreign emulator flags, a hand-patched binary — is tagged
`experiment`, recorded, and excluded from every statistic, because its outcome
is evidence about the probe and not about the tree. That mechanism exists
because of an almost identical incident on 2026-08-19, when a one-off `-cpu
host` boot died inside OVMF before our kernel loaded and zeroed a long clean
streak as a TIMEOUT.

The obvious move was therefore to reuse it: mark a host-OOM run `experiment`
and be done in one line. That was rejected, and `known-issues.md` had already
named why when it specified this work. **`experiment` means "invoked
deliberately under non-default conditions".** Widening it to also mean "or the
host hiccupped" makes it a flag with two meanings, which is a flag that can be
satisfied by whichever of them nobody was thinking about. The failure that
produces is not a mislabelled row: it is a *real* regression excused by a
predicate that was never about it.

So there are two predicates, `is_experiment` and `verdict == "HOST_FAIL"`, and
one function — `describes_tree` — that is the sole filter every statistic uses.
Two reasons to exclude, one place that knows the list, so the streak, the counts
and the medians cannot come to different views of what a boot is.

### What was rejected

| Option | Why not |
|---|---|
| Reuse the `experiment` flag | One flag, two meanings; a real failure could be excused by a predicate that was never about it (above). |
| Detect the host failure in the serial log | The **guest** writes the serial log. Any kernel that printed eleven particular words could then opt out of being blamed. The stream is the security boundary, not the words. |
| Make `HOST_FAIL` a *clean* verdict | It would pad the streak with boots that never finished. `known-issues.md` closure bars are counts of clean boots; manufacturing one closes real bugs. |
| Drop the row entirely | The reason a thing happened and what it looked like is exactly what stops it being misdiagnosed the second time. Recorded, excluded — never discarded. |
| Match host errors with a regex | A false positive here does not mislabel a row, it *removes a real failure from the counts*. Four literal substrings have a false-positive rate a reader can reason about; a regex does not. |

### Three properties that make it hard to weaken

1. **It is reachable only on positive evidence.** No stderr file, an unreadable
   one, an empty one, or one full of ordinary QEMU chatter all leave the
   existing verdict exactly as it was. Every error path in `read_qemu_stderr`
   returns `""`, and `""` can only ever leave a failure blamed on the tree —
   never excuse one. `PANIC` stays the default because the direction that fails
   safely here is the one that over-reports failures.
2. **The override runs downward only.** A host signature replaces a verdict
   that blames the tree and never one that clears it. Rewriting a `PASS` would
   *destroy* a real clean boot; a clean boot with a host warning on the side is
   still a clean boot, and is recorded as one with the warning noted.
3. **The evidence outlives the artefact.** `build/qemu-stderr.txt` is deleted by
   the next run, so the reason is copied into the row (`host_fail`) and printed
   to the console. A row that said only `HOST_FAIL` would be an assertion the
   next reader could neither check nor disbelieve.

### The one thing this costs

A `HOST_FAIL` row's *build* time was a real measurement — the compile finished
long before QEMU started — and it is dropped anyway. The justification is that
the memory pressure that killed the emulator is the same pressure the compiler
was running under, so the number describes a contended host rather than a
profile. Losing a sample understates nothing; keeping a contended one inflates a
median that later runs are judged against.

Fingerprints are skipped for `HOST_FAIL` for a sharper version of the same
worry. The matchers key on the *shape* of the exception in the log and do not
consult the verdict, and a host OOM lands on whatever allocation the kernel
happened to be making — so a host-killed boot can easily wear the shape of a
known bug. Recording that would file a recurrence of an issue that did not
recur, in the counter several `known-issues.md` closure bars are written in.
