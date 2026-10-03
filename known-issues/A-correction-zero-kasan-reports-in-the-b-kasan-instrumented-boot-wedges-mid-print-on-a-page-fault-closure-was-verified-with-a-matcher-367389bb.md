### [A] CORRECTION — "zero KASAN reports" in the `B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT` closure was verified with a matcher that could not fire — 2026-08-19

**Status:** OPEN

**In short:** When I closed that bug I wrote in `roadmap.md` that two instrumented
boots reached `BOOT_OK` with "zero KASAN reports". The check behind that claim
searched the boot log for the strings `BUG: KASAN` and `__asan_report`. Neither
string is emitted by this kernel anywhere — it prints `[kasan] CRITICAL:`. So the
check was guaranteed to find nothing regardless of what the boot did, and the
boot in fact produced 64 use-after-free reports (see
`B-KASAN-STALE-POISON-ON-LIVE-SLOT` above).

**What stands and what does not.** The *closure itself* stands: the bug being
closed was a spurious wedge report caused by a boot timeout calibrated for an
uninstrumented kernel, and two instrumented boots reaching `BOOT_OK` is the
evidence for that, independent of report counts. What does not stand is the
parenthetical "with zero KASAN reports", which has been struck in `roadmap.md`
and replaced with the real figure and a pointer here.

**Not retroactively checkable.** `build/serial-test.txt` is overwritten by every
boot, so the two earlier instrumented runs' logs no longer exist. Only the third
run's report count is known.

**Why it happened, and the guard.** `BUG: KASAN` is what *Linux* prints; I
matched the pattern I expected rather than the pattern the code emits. The
discipline that would have caught it costs one command: **before trusting a
negative grep, grep the source for the pattern** and confirm it can be produced
at all. This is the identical hazard `scripts/boot-history.py`'s docstring
already documents, which I had quoted approvingly earlier in the same session —
a warning is not a guard, and only a matcher checked against the emitter is.
