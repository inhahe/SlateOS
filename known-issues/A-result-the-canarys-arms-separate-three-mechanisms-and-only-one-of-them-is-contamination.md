### [A] RESULT — the canary's arms separate three mechanisms, and only one of them is contamination — 2026-08-19

**In short:** Six `--bench` boots with the new arm recording (78 sampled arm pairs)
answer the question the previous entry left open. The canary's odd readings are not one
phenomenon but three, and the two raw arms tell them apart at a glance where the single
derived number could not. One of the three is a genuine host disturbance; one is a
uniform speed change that is harmless; one is the instrument fooling itself.

**Correction to a premature reading.** The first run with arms caught a single +12.2%
sample whose `store` arm was bit-identical to the modal value while `nop` fell 4.6%, and
on that one sample it looked as though the canary's high readings were *generally* the
opposite of contamination. Five more runs refute that as a general claim: two of the
three large excursions collected since move **both** arms together and are real. The
n=1 reading was the rare case, not the rule. Recorded because the wrong version was
stated aloud before the additional runs existed.

**The three mechanisms.** Classifying all 78 samples against the modal arms
(`nop`=14156, `store`=19444) with a 0.5% threshold: 63 quiet, 13 both-arms, 2 one-arm.

| # | signature | n | centi produced | what it is |
|---|---|---|---|---|
| 1 | both arms scale by the **same** factor, −2.26% | 11 | 504–506 | uniform speed change |
| 2 | both arms rise together, +34% to +45% | 2 | 641, 766 | genuine host disturbance |
| 3 | **one** arm moves, the other does not | 2 | 579, 477 | instrument artefact |

Mechanism 1 is the historical ~5.04 cluster, and it is not an excursion at all: both
arms scale by one factor (mean divergence between the two arms across all 13 both-arm
samples is **0.41 percentage points**), so the difference scales with them and the
reported figure moves without anything having gone wrong. Mechanism 3 is the historical
~5.80 cluster — the one arm sample at 579 is a `nop`-only drop.

**Why a one-arm move must be an artefact, and why it is always downward.** The canary
value is a minimum over ~500 rounds. Minima are asymmetric:

- for a minimum to move **up**, essentially *every* round must be slower;
- for a minimum to move **down**, a **single** lucky fast round suffices.

So one arm's minimum catching one lucky round is cheap and sporadic — which is exactly
the discrete, position-independent pattern in the traces. Both observed artefacts are
one arm moving *down* (`nop` −4.59% with `store` +0.00%; `store` −2.06% with `nop`
+0.01%), and no one-arm *upward* move appears in 78 samples, as the asymmetry predicts.

The consequences differ by arm, and both are bad:
- a lucky **nop** round **inflates** the figure — a false contamination alarm (the 579);
- a lucky **store** round **deflates** it — a blind spot that *masks* contamination (the 477).

**The benchmarks do not see any of it, including the real disturbances.** For each
excursion, the eight benchmarks in its own sampling interval, against a median baseline
from the four quiet runs:

| excursion | arms | median move in its interval |
|---|---|---|
| 579 @ pos 72 | nop −4.6%, store ±0 | −0.02% |
| 766 @ pos 56 | nop +43.5%, store +44.9% | −0.26% |

For the artefact that is the expected result. For the +44% both-arms spike it is the
informative one: the disturbance is real, sustained enough to lift a minimum across the
canary's whole ~4.5 ms window, and still invisible in the benchmarks bracketing it —
because that window is a small slice of a sampling interval tens of milliseconds long.
Run wall times agree (113 s and 117 s for the two flagged runs against 112–117 s for the
clean ones), as do whole-run medians (−0.03%, −0.15%).

So a `contaminated` verdict from mechanism 2 is *true* about the host and *not*
evidence that the numbers in that interval are wrong. The canary is the more sensitive
instrument, which is the same conclusion RESULT P24 reached from the other direction.

**What this means for the verdict logic.** The current rule thresholds the derived
difference and therefore cannot separate any of these: it treats a harmless uniform
−2.26% scaling, a self-inflicted +12.2% artefact, and a real +44% disturbance as points
on one scale. With the arms recorded, the rule can key on the signature instead —
one-arm ⇒ discard the sample; both-arms-proportional ⇒ not an excursion; both-arms-up
⇒ a real host event, reported as such rather than as "these benchmarks are suspect".
Not yet implemented; the arms had to exist first, and only 6 runs carry them.

**Caveats.** Two samples per artefact/disturbance class is thin. The claim that the
historical ~5.80 cluster is mechanism 3 rests on one arm-bearing sample at 579; the
supporting argument is structural (mechanism 1 is observed only at −2.26% and could
reach 579 only as a proportional +12.2%, which has never been observed) rather than
purely empirical. The 79 pre-arms records cannot be reclassified at all.
