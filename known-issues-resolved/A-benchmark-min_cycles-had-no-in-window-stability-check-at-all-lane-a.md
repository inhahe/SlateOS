## Benchmark `min_cycles` had no in-window stability check at all (lane A)

**Status: FIXED 2026-08-15** (lane A). `bench::run` now splits each measurement
window into contiguous halves and reports the disagreement between their
minima as a new `<split>` column on the SCORE line;
`scripts/bench-history.py` withdraws any benchmark whose split is flagged from
the regression verdict instead of reporting it as a move.

**What was wrong.** `scripts/bench-history.py` diffs `min_cycles` boot-over-boot
and fails the build on a regression, but nothing checked whether the window that
produced `min_cycles` was quiet. `min` is robust to *spikes*; it is not robust to
a window that is uniformly busier than the boot it is being compared against —
the minimum of a busy window is simply the busy floor. `ab_interleaved`'s doc
comment already said this in as many words, and the `frame_owner` A/B that
motivated it had reported a 10826-cycle cost that vanished when interleaved. The
same failure mode applies to every history-tracked benchmark, and there was no
check on it whatsoever, which is the limiting case of a check that cannot fire.

Three disqualifications exist now and none substitutes for another:

| Check | Question it answers | Blind to |
|---|---|---|
| per-benchmark band | is a movement of this size normal *for this benchmark*? | a boot where the whole host was slow |
| canary | was the host busy? | a burst *inside* one benchmark's window — it samples between benchmarks |
| split-sample (new) | did the floor move *during this window*? | a window that was uniformly busy start to finish |

**The finding worth keeping: interleaving is the wrong tool here, and provably
so.** The first implementation split by parity — even iterations to set A, odd to
set B — by analogy with `ab_interleaved`, and it was caught before commit by
deriving what it actually detects. Consider the motivating case: load arrives
half-way through the window and stays. An even/odd split gives *both* sets
samples from the quiet part and from the busy part. Each set's `min` is therefore
the quiet-part floor. The two agree exactly, and the check reports a serene 0% on
precisely the window it exists to reject.

The generalisable lesson is that **the property that makes interleaving robust is
exactly the property that makes it insensitive.** Interleaving is correct for
`ab_interleaved`, where the question is "what does X cost *relative to* Y" and
the whole point is that ambient load must lift both arms equally so it cancels in
the difference. Here the question is the opposite — "did ambient conditions
change?" — and a construction designed to cancel out ambient change cannot
measure it. Contiguous halves work because the halves are *not* interchangeable;
that asymmetry is the signal.

Corollary for future checks: before building a self-check, write down the failure
it is for and trace that specific failure through the proposed construction. Both
designs here look equally reasonable in the abstract, and the difference is only
visible once a concrete failure is pushed through them.

**Known cost of the choice, accepted deliberately.** Halves reintroduce a bias
interleaving did not have: the first half is colder. `run`'s warmup is 10% of
iterations, enough for first-touch costs but not to saturate a slowly-filling
cache or TCG translation cache, so a benchmark that warms across its whole window
will show `min_first > min_second` on a quiet host every boot. That is not
treated as a false positive — a benchmark still warming during its own
measurement has no single noise floor, so its `min_cycles` is a function of how
far the warmup got, and diffing it compares two arbitrary points on a curve. The
flag says to lengthen *that benchmark's* warmup, not to loosen the gate. A
systematic flag is also self-announcing: it fires every boot, so it appears in
the suite-level count as a constant, where a noise flag comes and goes.

**Not yet calibrated.** `SPLIT_UNSTABLE_REL_PCT = 15` and
`SPLIT_UNSTABLE_ABS_CYCLES = 8` are guesses. The absolute floor exists because
the fastest entries land in the low tens of cycles, where one cycle of `rdtsc`
jitter is already several percent, and a gate that fires on those fires on
everything. Both constants are calibratable by construction: the per-benchmark
spread is printed *even when clean*, and the scorecard ends with a suite-level
`N of M checked entries unstable` line, so "68/70" (too tight) and "0/70" (too
loose) are both visible at a glance — neither is visible from a per-benchmark
flag alone. See the open todo to set them from a real `--bench` boot.

**A withdrawn measurement is not a passed one.** `bench-history.py` prints
flagged benchmarks under a separate `MEASUREMENT VOID` heading and counts them
apart from the ones that moved-but-within-band. Folding the two together would
let a suite where nothing could be measured read as a suite where nothing
changed. For the same reason `SplitCheck::NotChecked` — a hand-assembled
`BenchResult`, a derived per-switch figure, a run below `SPLIT_MIN_ITERATIONS`
— renders as `-` and never as a stability verdict in either direction.

### Follow-up 2026-08-16: the gate is calibrated, and the tally it was calibrated from was undercounting

**Threshold set to 30% (was a provisional 15%).** The first `--bench` boot
carrying the split column produced a strongly bimodal distribution over 91
measured windows: 65 at 0%, 10 at 1%, 10 at 2%, one each at 4% and 11%, two at
7% — then nothing at all until 74% (`page_alloc_zeroed_pool`) and 85%
(`vfs_stat_breakdown_full`). Since the 11–74% region is empty, every gate in it
flags the same two windows and the choice is purely about margin. 30% sits near
the geometric mean of the gap, ~2.7x above the worst benign window and >2x below
the smallest real disturbance.

The margin is deliberately wide because **that boot was itself contaminated** —
the canary fired and the dispersion instrument counted 18 stalled benchmarks — so
11% is a benign spread measured *under stress*, not on a quiet host. Calibrating
tightly against a stressed run guarantees spurious withdrawals on the next one.
The asymmetry reinforces it: a spuriously flagged window is withdrawn from the
regression verdict, so a too-tight gate erodes coverage *silently*, whereas a
too-loose one shows up as a suite-level count of 0.

Worth noting both flagged windows had `min_first < min_second` — the second half
slower — i.e. genuine within-window degradation, and specifically *not* the
warmup bias predicted for this design (which produces the opposite ordering).

**The bug found while calibrating: the suite tally could not see some of its own
flags.** The summary was computed by folding over the scorecard entries, but a
benchmark only becomes a scorecard entry if it calls `record()`/`track()`.
`page_alloc_zeroed_pool` calls `run()`, prints its line, and then dropped the
result with `let _ = result;`. The consequence is visible twice in one log: the
per-benchmark line reads `page_alloc_zeroed_pool: … (74% UNSTABLE)` and the
summary a few hundred lines later reads `worst spread 85%`. **A summary that
contradicts a line above it is worse than no summary**, because a reader who
spots the flag and then checks the total concludes the flag was retracted.

Fixed by moving the tally to the point the split is *measured* — `note_split()`
is called inside `run()` and `run_with_cache_info()`, immediately after the split
is computed and before the result can be discarded. This makes the escape
structurally impossible rather than merely fixed in the one place it was noticed:
there is no way to run a benchmark whose instability goes untallied, because the
tally happens inside the function that does the running.

`page_alloc_zeroed_pool` now calls `track()`, so a page-allocator fast path gets
regression detection it never had. It was not alone: 91 windows were measured but
only 70 reached the scorecard, so **21 measurements exist that no SCORE line, no
history entry, and therefore no regression check ever sees**. The scorecard now
prints a coverage line reporting that count, so the remaining gap is visible in
every boot instead of having to be rediscovered. Closing it for the other 20 is
open work — each needs a judgement about whether it is a real benchmark or a
diagnostic sub-measurement that should stay print-only.

**Candidate inventory for that remaining work** (static scan of `bench.rs`:
`run()`/`run_with_cache_info()` call sites per function, minus
`track()`/`record()`/`score()` call sites). Static name-matching is *not* the
authority here — it over-counts, because several benchmarks are recorded under a
different name than the function that measures them, and it cannot tell a
benchmark from a diagnostic. The runtime coverage line is the instrument; this
list is only a starting set to walk:

| Function | line | `run()` | recorded | gap |
|---|---|---|---|---|
| `bench_syscall_dispatch_breakdown` | 3275 | 6 | 0 | 6 |
| `bench_lock_primitives` | 4626 | 6 | 1 | 5 |
| `bench_pick_next_scaling` | 3149 | 2 | 1 | 1 |
| `bench_ipc_futex` | 3926 | 2 | 1 | 1 |
| `bench_vfs_stat_breakdown` | 4731 | 6 | 5 | 1 |
| `bench_net_veth_recv` | 5454 | 2 | 1 | 1 |

That totals 15. The scan also reports gaps in `run_all`, `timed`, `run`,
`print_scorecard` and `self_test`; those are **false positives** — dispatch and
infrastructure, not measurements — and are the clearest evidence that the static
count cannot be trusted on its own.

**Correction, same day — the prediction that stood here was wrong, and reading
the code is what corrected it.** It claimed the two clusters would decide
*differently*: that `bench_syscall_dispatch_breakdown`'s stages are diagnostics,
but that `bench_lock_primitives` "measures six genuinely independent primitives"
of which five were "real benchmarks that were simply never wired up." Only the
first half survived inspection.

`bench_syscall_dispatch_breakdown` is indeed a decomposition: it measures the
stages of one operation in isolation and prints an explicit `unexplained`
residual, while the parent `syscall_dispatch` is the thing actually scored. Its
six sub-measurements should stay print-only.

`bench_lock_primitives` is **the same shape, not the opposite one.** Its six
`run()` calls are `lock_raw_spin` / `lock_tracked` / `lock_no_lockdep` /
`lock_tracked_no_stats` — one lock operation under four instrumentation
settings, toggled via `lockdep::set_enabled` and `sync::set_tracking_enabled` —
plus `preempt_pair` and `rdtsc_pair`, the two suspected components measured
directly rather than differenced out. They feed a `lock overhead: total =
lockdep + preempt + rdtsc + unexplained` line. Recording them individually would
regression-track configurations the kernel never actually runs in.

And the sixth is already scored, under a **different name**:
`score("lock_uncontended", &tracked, 500)` records the measurement that `run()`
labelled `lock_tracked`. So the static table over-counts this function by one on
top of misclassifying the rest.

Two lessons, both about this entry rather than about the benchmarks:

- The static scan was labelled "not the authority" one paragraph earlier and
  then used as one anyway. A count of `run()` minus `record()` cannot see *what
  is being measured*, which is the entire question. The table above is retained
  as a navigation aid only — every row still needs the code read before it can
  be classified.
- The name mismatch is precisely the failure the runtime soundness check was
  written for, and it had a live instance on the first run. `lock_uncontended`
  sits on the scorecard having never been measured under that name, so a
  name-based diff reports `lock_tracked` as uncovered when it is fully covered.
  That is why the coverage instrument asserts the mismatch rather than assuming
  it away.

**RESOLVED 2026-08-16 (lane A).** Every measurement window is now either
recorded or declared a diagnostic; the coverage line should read `0 unjudged` on
every boot from here.

*The diff is no longer by name.* `BenchResult` gained a `seq` — its index into a
new `MEASUREMENTS` list, assigned by `note_measurement` and consumed by `record`
— so a window counts as covered precisely when *that window* was handed to
`record`. This matters more than the correction above implied: the name diff was
not wrong about one benchmark, it was wrong about **five**. `lock_tracked`→
`lock_uncontended`, `syscall_dispatch_task_id`→`syscall_dispatch`,
`heap_raw_alloc_free_64`→`heap_alloc_free_64`, `io_ring_nop_submit`→
`io_ring_nop`, `page_fault_anonymous`→`page_fault`. All five have been recording
history for weeks and all five would have been reported as uncovered, sending
the reader to wire up something already wired. The soundness check survives in a
stronger form: an out-of-range `seq` is counted in `SCORED_WITHOUT_MEASUREMENT`
and printed, because `seq` is a plain index and an invented one would mark the
*wrong* window covered — one mistake reported as two, in the direction that
hides work rather than inventing it.

*Thirteen windows declared diagnostics*, via a new `run_diagnostic()`: the six
`bench_syscall_dispatch_breakdown` stages and the five `bench_lock_primitives`
variants (both decompositions, as the correction established), plus
`vfs_stat_breakdown_full2` — a coherence re-measurement of a whole that is
already scored — and `self_test_nop`, which measures the harness rather than the
kernel. Declaring them is what lets the report reach zero; a report that nags
forever about settled decisions is one the reader learns to skip, which is the
same failure mode as an assertion that fires on every healthy boot.

*Eight real benchmarks were found genuinely unwired*, which is the part the
static table missed entirely because it had no row for them — they are `run()`
calls whose result was **discarded on the spot** (`run("x", …);` with no
binding), so no `record()` call existed to be counted as absent:

| Benchmark | Now | Why it mattered |
|---|---|---|
| `rdtsc_overhead` | `track` | The instrument every other benchmark is measured with. If it moves, every number in the suite shifts together and the comparator reads a suite-wide regression with no visible cause. |
| `page_alloc_zeroed_free` | `track` | Cold path; its hot-path sibling `page_alloc_zeroed_pool` was already tracked, so the zero pool's whole reason for existing — the gap between the two — had only one side recorded. |
| `heap_raw_alloc_free_512` | `track` | A regression confined to one size class is invisible in the 64 B number, which was the only one recorded. |
| `heap_raw_alloc_free_4096` | `track` | As above, and the size most likely to change allocator routing. |
| `compress_zero_page` | `track` | The compressor's best case, and the input the swap path hits most often. |
| `compress_repeating` | `track` | Paired with it: recording one leaves the compressor's *shape* — how steeply cost rises with entropy — unrecorded. |
| `hpet_read` | `track` | MMIO cost under every monotonic-clock read. Conditional; see below. |
| `futex_wait_mismatch` | `score` (500 ns) | The worst of the eight: it **already had a target and already graded itself against it**, in prose. Exactly the shape `ScoreEntry::target_ns` documents — a human-readable verdict is not a record. |

*A third lesson, then.* The correction above was about the static scan
misclassifying what it found. The deeper problem is that it could not find these
eight at all: a scan that works by pairing `run()` against `record()` sees
nothing when the result is dropped in the same expression. Only the runtime
instrument found them, because it counts windows rather than reading source.
`rdtsc_overhead` has been measured on every `--bench` boot for months and appears
in `bench/history.jsonl` zero times.

*One consequence accepted deliberately:* `hpet_read` is conditional, so a boot
without HPET will fail `test-bench-history.py`'s vanished-benchmark check. That
is the intended behaviour — a run missing a benchmark is a run not comparable to
its predecessor — and `page_alloc_zeroed_pool` was already conditional and
recorded, so this is precedent rather than a new hazard.

*The instrument then found a ninth on its very first boot* — and a worse one
than the eight, because it could not have been fixed by wiring alone.
`bench_pick_next_scaling` sweeps five run-queue depths (1, 8, 64, 256, 1024) and
ran **all five under the single name `sched_pick_next_isolated`**, scoring only
the deepest under the separate name `sched_pick_next`. Five history entries
under one key is not a series; it is four values overwriting each other. So the
four shallow points had to be *renamed* before they could be recorded at all:
they are now `sched_pick_next_d{1,8,64,256}` and tracked, while the deepest
keeps its scored name so its history stays unbroken.

Tracked, not declared diagnostics — the opposite call from the `_breakdown`
stages, and the distinction is about what the benchmark asserts. A
decomposition's stages are meaningless apart from their siblings: there is
nothing for a comparator to compare. A scaling sweep's points are each a
complete measurement of the same operation at a different load, and the claim
*is* the shape they trace. The in-kernel verdict only tests the two endpoints
against a 4x threshold with generous headroom, so a regression that bent the
middle of the curve passed it silently.

This is also the **second** concrete instance of the lesson above that a static
scan cannot be the authority here, and the first *false negative*: the audit
script searched forward from the `run()` call for a `score`/`track` naming the
same binding, and matched a `&result` belonging to a different function ~60
lines downstream. It reported the site as recorded. Only the runtime instrument
saw the four windows.

*The invariant is now asserted by the harness*, not merely printed.
`scripts/boot-test.sh` gained `check_bench_coverage()`, alongside
`check_liveness_failures()` and for the same reason: `BUG-LIVENESS-DEADLINE-
FALSE-FIRE` had required a clean liveness log in prose since 2026-07-27 while
runs violating it still exited 0. It fails on a non-zero `unjudged` count and on
the orphan-`seq` `NOTE` — the latter *even when `unjudged` reads 0*, since an
orphan marks some other window covered and so makes a clean count unbelievable.
Under `--bench` the coverage line is **required**: `run_all()` prints it before
`BENCH_OK` on both the deferred and the inline-fallback path, so "`BENCH_OK` but
no coverage line" means the instrument stopped running, which is precisely what
it exists to catch — treating that as a pass would reproduce this bug one level
up. Confirmed to fire on the real pre-fix log (it flags the `4 unjudged` line
and names all four windows), so a green boot means the fix holds rather than the
check being asleep.

Also fixed in the same pass: `run_all()` cleared `SCORECARD` but not the
`SPLIT_TALLY_*` atomics. Since the coverage figure was a subtraction of one from
the other, a second `run_all()` in one boot would not have degraded the report
but *inverted* it, fabricating a suite-sized gap. `reset_suite_state()` now
clears everything, and the per-window computation means the report no longer
depends on two totals agreeing.
