### [A] RESULT P24 — the idle host is not quiet, and "false positive" was the wrong name — 2026-08-19

**In short:** five benchmark runs with nothing else running on the machine. Two
of the five still showed a moment where memory access briefly cost 13% more than
usual, and the model dutifully reported it. Those reports were counted against
the model in the previous experiment as "false positives". They are not false;
the machine really did hiccup. What was wrong is the grader's assumption that
anywhere outside the deliberate load is guaranteed undisturbed.

Five `scripts/boot-test.sh --bench --host-load=idle` runs, no spinners, no load
window. Serial logs archived as `build/p24-run{1..5}-serial.txt`.

#### The scorecard — all three claims hold

| claim | registered | measured | |
|---|---|---|---|
| P24(a) quiet on idle | >= 3 of 5 runs flag zero | **3 of 5** (runs 2, 3, 4) | HOLDS |
| P24(b) position 72 not special | elevated in <= 2 of 5 | **0 of 5** | HOLDS |
| P24(c) no whole-run alarm | <= 1 `CONTAMINATED` | **0 of 5** | HOLDS |

Scored with the real model, not by hand:

```
run      baseline  flagged  positions
P24 r1     5.16       3     [31, 32, 33]
P24 r2     5.16       0     []
P24 r3     5.15       0     []
P24 r4     5.16       0     []
P24 r5     5.16       3     [7, 8, 9]
```

#### The self-contamination hypothesis is dead

Position 72 read clean in **all five** runs (5.16, 5.04, 5.15, 5.16, 5.16).
`vfs_throughput_16k_write/read` do not disturb the canary's reference. The
hypothesis I raised one entry above is refuted, and the P24(b) lean registered
against it — on the strength of P22 run 3 — was correct.

#### What is actually happening

Every canary sample ever recorded, across all archived traces, falls into two
groups with **nothing in between**:

- a baseline cluster spanning 5.04-5.22, overwhelmingly 5.16
- excursions, the smallest of which is 5.81

The gap between 5.22 (+1.2% over baseline) and 5.81 (+12.6%) is empty. So the
10% flag threshold is **not** miscalibrated — it sits cleanly inside that gap,
which is close to the best place it could be. I had started to write the
opposite; the distribution says otherwise and this corrects it.

The 5.81 value is worth noting on its own: it appears in three different runs
(P23 at position 72, P24 r1 at 32, P24 r5 at 8), always as a single isolated
sample, always at the same magnitude. A recurring discrete cost rather than a
continuous one — something quantised, landing at an arbitrary position.

So the model is not manufacturing anything. **On an idle host the canary
measures a real excursion roughly two runs in five, and the model reports it.**
That is the instrument working.

#### The consequence: the grader's ground truth is wrong, not the model's output

`grade-positional.py` scores any flag outside the load window's reach as a false
positive, with **tolerance 0**. That rule is only sound if the host is
undisturbed everywhere outside the window, and these five runs measure how often
that assumption fails: **2 of 5**, at ~3 benchmarks each.

A gate that fails on a coin-flip of host noise cannot distinguish a good model
from a bad one, which is the only thing it exists to do. P23's
`FAILED (misplaced)` verdict is therefore **not evidence against the model** —
its two outside-reach flags at positions 72-73 are indistinguishable from the
spikes seen here at positions 32 and 8 with no load at all.

**P23's recorded verdict stands as it was graded** — results are not rewritten
after the fact. What changes is how it is read, and that is what this entry is
for.

#### The harder finding, which no threshold fixes

The idle-host excursion is +12.6%. The smallest *genuinely loaded* sample in
P23's window was +13.2% (position 40). Those two are not separable by magnitude:
a threshold raised to suppress the noise would also have discarded real signal,
and only P23's +44.8% sample at position 48 sits clearly above the noise
population.

So on this host, a 6-spinner load and an ordinary idle hiccup produce
overlapping per-sample excursions. The model's discrimination is limited by
that overlap, not by the sampling interval and not by the median-baseline cliff.
Raising `POSITIONAL_NOTE_PCT` would trade a false positive for a false negative
at roughly one-for-one and is **not** the fix.

The fix worth pursuing is at the source: find what the recurring 581-centicycle
event *is*. It is discrete, repeatable in magnitude, and position-independent,
which is the profile of a specific mechanism rather than of thermal or
scheduling drift. Eliminating or excluding it would raise the model's floor
without costing any sensitivity. Recorded as the next investigation rather than
guessed at here.
