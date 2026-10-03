### [A] RESULT P22, first attempt — VOID: the stimulus never landed where the label said — 2026-08-18

**In short:** we deliberately slowed the computer down during a known stretch of
the benchmark suite, to see whether the drift model would point at that stretch.
The tool that was supposed to apply the slowdown applied it too late and left it
on too long, so the stretch we *labelled* as slowed was in fact untouched and a
later stretch we labelled clean was the one that suffered. The grader, which
took the label on trust, then reported that the model had looked in the wrong
place — when in fact the model had pointed straight at the real disturbance.
**This run grades nothing about P22(a) and must not be cited as evidence against
it.** It does carry real evidence for P22(b), which does not depend on the label.

Command, for the record:

```
scripts/canary-load-test.sh 6 --load-at=cp_try_wait_empty --load-until=net_veth_recv
```

Serial log preserved at `build/p22-loaded-serial.txt`, boot log at
`build/p22-loaded-boot.log`. Both are kept because the run's *timings* are
sound — it is only the label on them that is wrong, and the timings are what the
P22(b) finding below rests on.

#### What was claimed, and what actually happened

The label said positions 30–55. Measured afterwards against the previous clean
run — from the benchmark timings themselves, which are independent of both the
canary (the instrument under test) and the load script (the claim under test):

| region | positions | n | best-case | mean | |
|---|---|---|---|---|---|
| before the window | 0–29 | 30 | ×0.97 | ×0.88 | clean by construction |
| **the labelled window** | 30–55 | 26 | **×0.99** | **×1.12** | **not disturbed** |
| beyond the window's reach | 64–85 | 22 | **×1.40** | **×2.30** | **this is where the load went** |

The load did not arrive in the window at all, and it was still running two
sampling intervals past the point it was supposed to have been removed.

#### Why — and the reason is arithmetic, not carelessness

The whole 86-benchmark suite is a **~6 s tail of a ~131 s boot**, occupying
about 350 of the run's 27,847 serial lines. A 26-benchmark interior window is
therefore roughly **two seconds** long. Against that:

- the bash follower polled with `sleep 1`, so the trigger was noticed up to a
  second late — half the window, gone before anything started;
- the load was then six `python -c` spin loops started as **new processes**
  under MSYS, each of which is a fresh interpreter startup, so the first spinner
  reached its loop later still;
- and the release at `--load-until` paid the same poll latency again at the far
  end, by which time the suite had moved on.

Net effect: the load switched on after the labelled window had largely passed
and switched off well into the region labelled clean. Nothing in the script was
wrong in the sense of doing something other than it said; the costs it paid were
simply larger than the thing it was measuring. That is the general shape of the
error worth remembering — **a controller whose latency is comparable to the
window it controls does not control that window.**

#### The second failure, which is the dangerous one

The grader took `--load-at`/`--load-until` as ground truth. Every flag the model
raised in the escaped region was therefore counted as a **false positive**, and
the run was reported `FAILED (misplaced)` — 22 of 44 "provably-clean" benchmarks
flagged. Those 22 flags were the model **correctly reporting a real
disturbance** that the run's own label denied.

A grader that can convict a model for being right is worse than no grader. The
general rule, which cost a whole run to learn and is now written at the top of
`scripts/grade-positional.py`:

> You cannot grade "did the model find the disturbance" until you have
> established "there was a disturbance to find."

#### What was changed before re-running (commit `2916a3d2f`)

- **`scripts/canary-load.py`** replaces the bash follower. Spinners are spawned
  *before* QEMU boots and park on a semaphore, so firing the load is a kernel
  wakeup rather than six interpreter startups; the log is tailed in-process at
  0.1 s; and a JSON record states what the controller actually did. The spinners
  watch their own parent and exit if it dies, because an MSYS `kill` of a native
  Windows process is `TerminateProcess` and runs no handler — the record is
  written by asking the controller to stop via a file, never by signalling it.
- **`scripts/grade-positional.py`** now measures the stimulus before it grades
  the model, and returns `UNGRADED` — whatever the model did — unless both:
  the labelled window ran measurably slower than the run's own untouched prefix
  (*the load reached it*), and the region beyond the load's reach did not
  (*the load stayed put*). Re-run against the void log above, it now says
  `UNGRADED — the stimulus never reached the window`, and prints all three
  regions so the reason is visible rather than inferred.
- **The thresholds came out of the history, not out of a principle.** The first
  draft reused the model's own 10% reporting threshold, which is *below the
  instrument's noise floor*: across the 60 clean release runs on this host, 38%
  of benchmarks move ≥10% on the mean from one run to the next. Requiring both
  the best-case median (≥1.15) and the mean median (≥1.30) to fire costs **1
  false positive in 236 clean region-pairs**, against a real disturbance that
  measured 1.40 and 2.30. `scripts/test-grade-positional.py` re-derives that
  rate from the real `bench/history.jsonl` on every run and fails if it drifts
  above 5%, so a suite that gets noisier cannot quietly turn the gate back into
  a rubber stamp.
- **The baseline is the single previous clean run**, not a median of eight. An
  8-run window spans eight commits of kernel changes, and those are not spread
  evenly over the suite, so different *regions* acquire different apparent
  drifts: it put the untouched prefix at ×0.70 where the previous run put it at
  ×0.88 — an error larger than the disturbance being measured.

#### The one real finding: evidence for P22(b)

P22(b) — *"the benchmarks the model flags will be inflated relative to their own
historical medians, and the ones it does not flag will not"* — makes **no
reference to where the load was applied**, so the broken label does not touch
it. On this run:

| | n | best-case | mean |
|---|---|---|---|
| flagged by the model | 34 | ×1.31 | ×1.84 |
| not flagged | 52 | ×0.97 | ×0.92 |

That is a clean separation, and it is the first evidence of any kind that the
model's flags correspond to benchmarks that really did run slowly.

**Three limits on how far it goes**, all of which must survive with the number:

1. **It is one observation, not 34.** The flagged benchmarks form a single
   contiguous stretch at the end of the suite, because the load was one
   contiguous stretch. "The model flagged the slow region" and "the model
   flagged 34 benchmarks that were slow" are the same fact counted twice.
2. **The magnitude does not track.** Correlating each benchmark's canary factor
   against its measured inflation gives Pearson r = **+0.30** on the best case
   and **+0.03** on the mean. The model's *binary* judgement carries the signal;
   the size of the factor it prints carries almost none. That bears directly on
   P22(c) and on §229's second objection to correcting automatically — a
   correction multiplies by the factor, and the factor is what does not
   correlate.
3. **It says nothing about P22(a)**, which is the attribution claim the printed
   line actually makes, and which needs a window whose ground truth is sound.

#### Status

- **P22(a): UNGRADED.** Re-run required, with the fixed stimulus.
- **P22(b): supported by one run, weakly** — see the three limits above.
- **P22(c): refuted-leaning.** r = +0.03 on the mean is not a ratio varying
  systematically with benchmark kind; it is a ratio carrying no information.
  Not recorded as a result because it was measured on a run whose stimulus was
  uncontrolled, and it deserves a run where the disturbance is known.

**The standing advice does not change**: treat the attribution line as a
hypothesis about which benchmarks to re-run, not as evidence about any of them,
and do not build a correction on it.
