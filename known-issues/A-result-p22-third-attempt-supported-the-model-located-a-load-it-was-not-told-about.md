### [A] RESULT P22, third attempt — SUPPORTED: the model located a load it was not told about — 2026-08-19

**In short:** the benchmark suite has a feature that tries to guess which of its
own measurements were disturbed by other activity on the machine. Twice we tried
to test it by deliberately slowing the computer down during a known stretch of
the suite, and twice the test itself broke before it measured anything. The third
attempt worked. The feature correctly picked out benchmarks inside the disturbed
stretch and flagged **none** of the 58 benchmarks we can prove were undisturbed.
One scary-looking number in the output — a monitor claiming the computer got
*faster* while we were loading it — turned out to be a broken thermometer, not a
real effect, and is fixed.

#### The run

```
scripts/canary-load-test.sh --at crypto_sha256_1KiB --until vfs_stat_3comp
```

Load: 6 spinner processes, held 3.33 s of an 8.85 s suite (37.6%), over suite
positions 60–71 (12 of 86 benchmarks). Fired and released on the correct
benchmarks; the window had both edges, which is what the first two attempts
lacked.

| Region | best-case | mean | reading |
|---|---|---|---|
| clean prefix (60 benchmarks before the window) | ×0.97 | ×0.87 | the run's own speed, the reference |
| **loaded window** (12) | **×1.34** | **×2.58** | **DISTURBED** (needs ×1.15 / ×1.30) |
| beyond the model's reach (6) | ×1.03 | ×1.07 | clean |

```
VERDICT: SUPPORTED -- 7 of 12 loaded benchmarks flagged, 0 of 58 clean ones
  sensitivity     : 7 of 12 loaded benchmarks flagged
  localisation    : 7 of 7 flagged benchmarks were within reach
  false positives : 0 of 58 provably-clean benchmarks flagged
```

The stimulus is measured **from the benchmarks**, independently of both the model
under test and the load script's own claims — the distinction the first attempt
was lost on.

#### Why 7 of 12 is the ceiling, not a shortfall

The obvious objection is that 7/12 sensitivity looks mediocre. It is in fact the
instrument's resolution limit, and the run's own canary trace shows why:

```
[bench] CANARY-TRACE 0:5.17 8:5.16 16:5.16 24:5.16 32:5.16 40:5.16
                     48:5.04 56:5.16 64:6.17 72:5.16 80:5.16
```

The canary is sampled **every 8 benchmarks**. The window is positions 60–71, so
exactly **one** sample (position 64) falls inside it — 56 is before the window,
72 is after. That one sample is the only elevated reading in the whole run
(6.17 against a 5.16 baseline, ×1.196), and it is correctly inside the window.

So the model was working from a single elevated sample and attributed 7
benchmarks to it. It cannot attribute all 12 without a second sample inside the
window. **Sensitivity here is bounded by the sampling interval, not by the
model**; a fair sensitivity test needs a window several times wider than 8
benchmarks, or a finer canary interval. What this run does establish cleanly is
the claim P22(a) actually makes — *attribution*: 7 of 7 flagged benchmarks were
within reach of the real window, and nothing outside it was flagged.

The 0-of-58 false-positive result is not resolution-limited and is the stronger
half of the finding.

#### The x0.78 that nearly sank the write-up

The load controller's host-side canary reported **x0.78** — a fixed unit of work
on the host completing *faster* while six CPU burners ran. Backwards, and not
something to write up around.

It is an artefact of the statistic. Measured directly on this 12-core host, 12
back-to-back trials, six spinners verifiably alive in every one:

| statistic | median ratio | range | inverted (<1.0) |
|---|---|---|---|
| **median** (what was being reported) | ×1.106 | ×0.784 .. ×1.526 | **3 of 12** |
| min | ×1.004 | ×0.999 .. ×1.036 | 0 materially |
| trimmed 25% | ×1.005 | ×0.999 .. ×1.130 | 0 materially |

One trial returned **×0.784** — run 3's number, from a load that was applied
correctly. A separate zero-spinner control produced **×0.77** with no load at all.
The median's run-to-run spread (1.95×) is simply wider than the effect it was
being asked to detect.

Recomputing run 3's canary best-case gives **×1.018**, and that is the *correct*
reading: with 12 cores and 6 spinners the probe keeps a free core, so it should
see almost nothing. Two ruled-out explanations, for the record: it is **not** a
CPU frequency ramp (`min` was identical to three decimals across idle/loaded/idle
— 0.247 / 0.246 / 0.246 ms), and **not** a baseline-composition artefact
(comparing against temporally adjacent samples made it *worse*, ×0.716, not
better).

**Consequence: the host canary cannot answer "was the load applied", even in
principle, at this spinner-to-core ratio.** It was the wrong instrument for that
question and is now demoted to descriptive colour.

#### What was fixed

1. **Occupancy replaces inference.** Each spinner publishes its own
   `process_time` into a shared cell; the controller snapshots them at fire and
   at release. Six spinners across 3.3 s should burn ~20 s of CPU, and the ratio
   of burned to available is the answer. It needs no baseline, cannot be
   confounded by whatever else the host is doing, and does not care how many
   cores the machine has. Verified end-to-end at 102% on a live 4-spinner run.
2. **Best-case promoted, median demoted.** `inflation` now carries the best-case
   ratio; the median is retained as `median_inflation`, explicitly labelled
   unreliable. Old records are detected and labelled `(median, unreliable)` on
   display rather than silently compared against new ones.
3. **`load-not-applied`** joins the two existing confessions. Occupancy below
   0.5 voids the run, admissible under the same rule as the others: the
   controller reporting from the spinners' own clocks that they never ran is an
   *admission* of an empty window, not a claim to have filled one. The grader
   derives it independently rather than trusting the field.
4. **A test that had quietly stopped testing anything.** `test-grade-positional`
   checked its "run 2's broken record" case against `build/canary-load-record.json`
   — the *live* output path, overwritten by every run. It had become a test of
   whichever run happened last, and duly "failed" once run 3 succeeded there. Now
   reads the preserved copy.
5. **Sabotaged boots are labelled.** `canary-load-test.sh` restores
   `bench/history.jsonl` (this run's numbers are deliberately poisoned) but not
   `boot-history.jsonl`, so a loaded run left an anonymous 117 s entry that a
   future reader would take for a boot-time regression. Now labelled
   `p22-loaded`; `wall_seconds` there is displayed only, never used as a
   baseline, so this is legibility rather than correctness.

#### Note on the kernel delta

This run carried one lane-A kernel change (the `MEASURED-AS` line, emitted in
the post-suite reporting path). Its clean prefix measured ×0.97 best-case,
matching run 2's ×0.96 — the change is inert with respect to the benchmarks, as
its position in the code implies.

#### Status

- **P22(a): SUPPORTED.** Attribution works: 7 of 7 flagged benchmarks inside the
  real window, 0 of 58 provably-clean benchmarks flagged. This supersedes
  "still UNGRADED" above.
- **Sensitivity: not yet fairly measured.** 7 of 12 is bounded by the canary's
  8-benchmark sampling interval, which put a single sample inside the window. A
  window several times wider than 8 benchmarks is needed to measure it.
  **Registered as PREDICTION P23 below** — a 32-benchmark window carrying four
  samples, with the pass threshold derived from the model's own arithmetic
  *before* the run, precisely so this paragraph cannot be marked correct after
  the fact.
- **P22(b): unchanged** — supported by one run, weakly (see the first attempt).
- **P22(c): unchanged** — refuted-leaning.
- **No correction is applied to any recorded value.** §229 stands: the model may
  now say *where*, which is all the printed line claims; it is not yet licensed
  to say *by how much*.
