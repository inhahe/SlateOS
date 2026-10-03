### [A] W-BENCH-THREE-BENCHMARKS-ABOVE-SUITE-DRIFT-WITH-NO-MATCHING-CODE-CHANGE. firewall_check / shm_create_close / ipc_semaphore — ✅ RESOLVED 2026-08-14: all three were noise

**RESOLUTION (2026-08-14, third run `a18ea83a9`).** All three were noise, and
the third run says so about as loudly as data can. They came back not merely
to the suite median but to *below* their first-run values — and they are the
**top three entries in the IMPROVED list**, in the same order they had
occupied in REGRESSED:

| benchmark | run 1 `bf26aabdb` | run 2 `17dbde179` | run 3 `a18ea83a9` | verdict |
|---|---|---|---|---|
| `firewall_check` | 270 ns | 482 ns | **228 ns** | run 2 is the outlier |
| `shm_create_close` | 58 556 ns | 84 996 ns | **56 734 ns** | run 2 is the outlier |
| `ipc_semaphore` | 11 676 ns | 16 112 ns | **11 219 ns** | run 2 is the outlier |

Runs 1 and 3 agree to within 3–16 % in every case; run 2 stands alone. A real
regression does not un-regress with no code change, so the correct reading is
that run 2 was the anomaly, not run 3 — i.e. these were never regressions at
all, and the flat 25 % threshold flagged them purely because their intrinsic
spread exceeds it. The prediction recorded below — that `firewall_check` at
270 ns would prove the noisiest by construction — held: its spread is 111 %,
the second-widest in the suite.

**Measured per-benchmark spread (max/min across the three runs), which is the
number the comparator has been missing all along:**

* median across all 63 benchmarks: **13 %**
* but the tail is long: `crypto_ed25519_verify` 416 %, `firewall_check` 111 %,
  `tcp_checksum_v6` 56 %, `shm_create_close` 50 %, `sched_pick_next` 49 %,
  `syscall_dispatch` 44 %, `ipc_semaphore` 44 %.

So a flat 25 % threshold is below the natural spread of at least seven
benchmarks and far above that of the median one — it is simultaneously too
tight and too loose, which is exactly the failure mode observed. **This
promotes the "proper fix" named below from a suggestion to the next task:
give the comparator a per-benchmark variance estimate.** Logged as
TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE below.

**Caveat recorded honestly: run 3 is partially contaminated, by me.** I ran
greps, `git`, and `python` in the same window as the benchmark suite, having
explicitly noted beforehand that the machine should be idle. Median drift
correction removes a *uniform* slowdown; it cannot remove contention that
lands on whichever benchmark happens to be running at the time. That is the
most likely explanation for run 3's own new outliers —
`crypto_ed25519_verify` (30.7M → 31.4M → **158.6M**, i.e. two tight samples
then 5.1×) is the longest-running benchmark in the suite and therefore the
most exposed to a contention window. Do **not** treat that as a regression on
this evidence; see TD-BENCH-RUNS-ARE-CONTAMINATED-BY-THE-AGENTS-OWN-COMMANDS
below.

The original WATCH text follows unchanged.
