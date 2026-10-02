## §265 — The contamination-canary check gets a measured noise distribution before its tolerance is touched

**Date:** 2026-08-21
**Decided by:** Operator (Claude recommended this option; operator: "i'll go
with your recommendation")
**Lane:** A

**In short:** We have a tool that watches for the machine getting busy while
benchmarks run, so that a benchmark slowed by *other work on the PC* isn't
mistaken for a real regression. The check that verifies this tool works fails it
if it points anywhere unexpected at all — zero tolerance — and we have now
measured that the machine hiccups on its own in 2 runs out of 5 with nothing
running. So the check can fail a perfectly good tool on a coin flip. The
decision is to run 20+ idle benchmark rounds first and set the rule from the
resulting distribution, then replace zero-tolerance with a rule that fails on a
*shifted band* rather than on isolated spikes.

### Why not just relax it now

Because five runs is enough to prove the assumption is wrong and far too few to
calibrate anything — picking a number from 5 samples is exactly how the original
zero got there. The 20-run sweep costs about 45 minutes of machine time.

### Why a band, not a count

An isolated spike and a wrongly-placed window are different faults. "Reported a
stray hiccup somewhere" is the tool working correctly on a noisy host; "found
the window but in the wrong place" is the fault the check exists to catch. A
count-based tolerance cannot tell them apart; a band-shift rule can. It is more
code, and the band/spike boundary needs its own justification from the sweep.

### The conflict-of-interest note, kept deliberately

The rule being relaxed is the one that returned `FAILED` on my own experiment
three hours before the question was filed. Changing a gate immediately after it
fails you is indistinguishable from moving the goalposts however sound the
reasoning, which is why this went to the operator rather than being decided
unilaterally. The part with no such hazard was already done: the grader prints
the measured idle-host rate beside the count. **No verdict was changed.**

Evidence: `known-issues.md` RESULT P23, RESULT P24.
