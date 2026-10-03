### [A] RESULT P23 — all four registered claims falsified, and the model was right every time — 2026-08-19

**In short:** the experiment predicted the contamination model would catch 75% of
the benchmarks it disturbed. It caught 50%, flagged two benchmarks it should not
have, and tripped a whole-run alarm that was predicted to stay quiet. All four
predictions were wrong. The model, however, did exactly what its arithmetic says
it does — every number it produced is reproducible from its own recorded input.
What was wrong was the *prediction*, which assumed a kind of disturbance the
machine does not actually produce.

Run: `scripts/canary-load-test.sh --at io_ring_nop --until crypto_poly1305_1KiB`,
6 spinners, 98% occupancy, window landing at positions 32-63 as registered.
Serial log archived at `build/p23-run-serial.txt`. Boot 123, streak 22, PASS.

#### The scorecard

| claim | registered | measured | |
|---|---|---|---|
| P23(a) sensitivity | >= 24 of 32 (75%) | **16 of 32 (50%)** | FALSIFIED |
| P23(b) false positives | 0 of 38 | **2 of 38** (`vfs_throughput_16k_write@72`, `..._read@73`) | FALSIFIED |
| P23(c) localisation | 100% | **17 of 19 (89%)** | FALSIFIED |
| P23(d) whole-run verdict | `Canary OK` | **`CONTAMINATED`** (spread 47%, tolerance 25%) | FALSIFIED |

Grader's verdict: `FAILED (misplaced)`.

#### The model is not what failed

The whole trace it worked from is one line:

```
CANARY-TRACE 0:5.16 8:5.16 16:5.16 24:5.05 32:5.16 40:5.84 48:7.47
             56:5.17 64:5.16 72:5.81 80:5.17
```

Median baseline 5.16. Three samples clear the 10% flag threshold — 40 (x1.132),
48 (x1.448) and 72 (x1.126) — and interpolating between them reproduces the
grader's output exactly: **19 flagged, 16 inside the window, 17 within reach, 2
outside at 72 and 73.** Not approximately; identically. There is no defect here
to fix. The model transcribed its input faithfully.

#### Why sensitivity was 50% and not 75%

The 75% figure came from `scripts/positional-model-limits.py`, which models the
window as **uniformly** elevated. Its own docstring disclaims this in as many
words: *"no real load does -- the real one ramps within a sampling interval at
each edge."* That disclaimer turns out to understate the problem. The window's
two interior samples were elevated (40, 48); **its two edge samples, 32 and 56,
read baseline** — 5.16 and 5.17, indistinguishable from an idle host — while the
spinners were at 98% occupancy throughout.

So the load was unquestionably applied and the probe still could not see it half
the time. With 6 spinners on a 12-core host the probe frequently lands on a free
core; the run's own summary says so (`host canary x0.99 ... near x1.00 is
expected when the host has more cores than spinners`). The stimulus is **spiky
at the canary's sampling resolution**, not a plateau, and a 4-sample window that
happens to catch 2 spikes yields exactly the 50% observed.

This is the more useful finding of the two: the width table in the entry above
is arithmetic, and arithmetic is all it was ever claimed to be. Sensitivity
against a *real* load is bounded not by the sampling interval but by how often
6 spinners actually collide with the probe — which no synthetic trace can
predict, and which this run measures at roughly half.

#### The two "false positives" are a real signal

Positions 72 and 73 are `vfs_throughput_16k_write` and `vfs_throughput_16k_read`
— the two heaviest I/O benchmarks in the suite. The canary read **5.81 at
position 72** while 64 and 80 both read baseline. The load was released at
position 63, and position 64 is clean, which rules out spinner teardown.

So the elevation at 72 is real and is not the load. The most likely explanation
is that those two benchmarks disturb the host enough to inflate the canary's own
reference — the suite contaminating its own instrument. If that is right, then
the grader's "provably clean" region is not clean, P23(b) was **unfalsifiable as
written**, and the model was penalised for correctly reporting a disturbance
whose only fault was not having been caused by the load script.

**This is a hypothesis with one supporting run.** Position 72 is elevated in 3
of the 4 archived traces, but those runs carried loads in different places, so
they do not separate the two explanations. The control that does is a bench boot
with no load at all — registered as P24 below.

#### P23(d) did not test what it was meant to

The claim was that widening the window changes extent and not depth, so the
spread verdict — blind to extent — would stay quiet. It did not: spread came out
at 47% against a 25% tolerance.

But the reason is that **depth was not held constant**, which the prediction
simply assumed it would be. Run 3's worst sample was 6.17; this run's was 7.47.
Nothing in the design pins the peak, so the two runs differ in depth *and*
extent, and the run therefore says nothing either way about the extent-blindness
claim. That claim stands on the run-3 evidence alone, undisturbed and untested
here. Registering P23(d) at all was the error: it was a prediction about a
quantity the experiment did not control.

#### What this costs, and what it does not

Nothing recorded is corrected on the model's say-so (design-decisions.md S229
stands), so a 50%-sensitive model with a self-contaminating reference has not
put a wrong number in any benchmark history. What it does mean is that
**`grade-positional.py`'s attribution should be read as a lower bound on extent,
never as a boundary** — it found the middle of a 32-wide window and missed both
edges, and the same run flagged two benchmarks outside it for unrelated reasons.
