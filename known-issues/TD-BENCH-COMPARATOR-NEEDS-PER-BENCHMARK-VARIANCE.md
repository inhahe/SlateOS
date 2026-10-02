### [A] TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE. A flat 25% threshold is below the natural spread of seven benchmarks and far above the median one's — 2026-08-14 — OPEN

**Where:** `scripts/bench-history.py`, `diff()` / `THRESHOLD_PCT`.

**What.** The comparator flags a benchmark when its drift-corrected change
exceeds a fixed ±25 %. Three runs of history now show that a single flat
threshold cannot work, because the suite's per-benchmark spread (max/min
across runs, *with no code change explaining it*) ranges over an order of
magnitude:

* median benchmark: 13 % spread → 25 % is far too loose; a genuine 20 %
  regression here would pass unnoticed.
* `crypto_ed25519_verify` 416 %, `firewall_check` 111 %, `tcp_checksum_v6`
  56 %, `shm_create_close` 50 %, `sched_pick_next` 49 %, `syscall_dispatch`
  44 %, `ipc_semaphore` 44 % → 25 % is far too tight; these produce false
  positives every single run.

Two investigation cycles have now been spent hand-adjudicating false
positives thrown by this threshold (see the RESOLVED entry above and
B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION). That is the
signal to fix the estimator rather than keep adjudicating its output.

**Proper fix.** Give each benchmark its own noise band derived from its own
history, and flag only moves outside it. Concretely: keep the existing
whole-suite median drift correction (it removes the common factor correctly
and is not in question), then compare the drift-corrected change against a
robust per-benchmark dispersion — median absolute deviation of the log-ratios
across the recorded runs — rather than a constant. Retain a flat *floor* so
that a benchmark with an implausibly tight history cannot be flagged on a
sub-noise move, and require a minimum number of runs (the existing
`MIN_SAMPLES_FOR_DRIFT` precedent) before the per-benchmark band is trusted,
falling back to the flat threshold until then.

**Test it the same way the drift fix was tested:** replay the estimator
against the recorded `bench/history.jsonl` and confirm it drops the three
now-known-noise entries while still flagging a deliberately injected
regression. Do not ship it on reasoning alone — that is the mistake this
whole thread of entries keeps documenting.

**Update 2026-08-14 — the fix above is DATA-BLOCKED; the unblocking step has
landed.** Attempting the implementation established that it cannot be built
*or* validated yet, which is worth recording so the next attempt does not
rediscover it:

* The MAD-of-log-ratios estimator needs the spread of each benchmark across
  runs. `bench/history.jsonl` holds **3** records, all from one host — which
  yields **2** consecutive run-over-run residuals per benchmark. A median
  absolute deviation over 2 points is not an estimate of anything; with
  residuals of `{+2 %, +406 %}` it returns ~204 %, a band so wide it would
  flag nothing, and one more run could as easily make it 2 %, a band so tight
  it flags everything. A minimum-runs gate (the fix's own proposal) would
  simply keep it disabled.
* The test requirement above is therefore *also* unsatisfiable today: with 3
  records there is no held-out data to replay against. Shipping it anyway
  would be exactly the "on reasoning alone" failure the entry warns about, so
  it was not shipped.

**What landed instead** (commit alongside this update): the harness now emits
and records a per-benchmark **dispersion** figure, which supplies the missing
noise scale *from a single run* rather than requiring history to accumulate.
`kernel/src/bench.rs::print_scorecard` extends the machine-readable line to

```text
[bench] SCORE <name> <min_ns> <target_ns> <PASS|OVER> <mean_ns> <iters>
```

and `bench-history.py` stores `mean_ns` / `iterations` as sibling maps in each
record. The trailing pair is optional in the parser, so the 3 existing records
still load — `scripts/test-bench-history.py` pins that down against the real
history file, because those records are ~9-minute boots on commits that are
now in the past and cannot be regenerated.

`mean/min` is a genuine per-benchmark noise scale and not a proxy for one: the
scorecard reports `min` because it is the least-contaminated estimate, but a
benchmark whose mean sits at 1.05× its min took a clean measurement on nearly
every iteration, while `dashboard_api_status` at 6.6× (160.4 ms mean vs 24.4 ms
min) was interrupted on most of them — so its reported min is whichever
iteration happened to dodge the interference, and is correspondingly fragile
run-to-run. That is precisely the property the band needs to size itself by,
and the two entries plainly should not share one threshold.

**Remaining work, in order.** (1) Accumulate ≥6 same-host records — this is a
by-product of ordinary benchmarked boots, not a task. (2) Validate the
`mean/min` → run-over-run-sigma mapping *empirically* against those records
before using it; the causal story above is plausible but the coefficient is
not known, and inventing one would just build a new false-positive generator
with more decimal places. (3) Then implement the band, preferring the
historical MAD where enough runs exist and falling back to the dispersion
prior where they do not. Do **not** skip step (2).
