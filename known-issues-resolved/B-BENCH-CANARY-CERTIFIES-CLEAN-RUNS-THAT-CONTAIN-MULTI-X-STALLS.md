### [A] B-BENCH-CANARY-CERTIFIES-CLEAN-RUNS-THAT-CONTAIN-MULTI-X-STALLS. All three runs it passed had 5–8 benchmarks stalled ≥5x — MITIGATED 2026-08-14

**Where:** `kernel/src/bench.rs` (`maybe_canary_sample`, `CANARY_SAMPLE_EVERY = 8`)
and `scripts/bench-history.py` (the `Canary OK` verdict).

**What.** The mid-suite canary has never once fired. That was read as "the host
has been quiet"; it is not what the data says. Cross-checking each run's canary
verdict against the per-benchmark `mean/min` recorded in the same run:

| run | canary verdict | benchmarks with `mean/min` ≥ 5x |
|---|---|---|
| `be167dd90` | clean (endpoints 97%) | **8** — `ipc_channel` 23x, `page_alloc_free` 19x, `syscall_dispatch` 16x, `pick_next` 16x, `context_switch` 11x, `crypto_ed25519_sign` 8x, `dashboard_api_status` 8x, `ipc_channel_sync` 6x |
| `5a2002bac` | clean (spread 2%) | **5** — `page_alloc_free` 24x, `vfs_stat_deep` 15x, `vfs_stat_3comp` 12x, `crypto_ed25519_verify` 10x, `vfs_throughput_16k_write` 5x |
| `f74f97b6d` | clean (spread 16%) | **6** — `context_switch` 21x, `vfs_stat_deep` 16x, `vfs_stat_3comp` 14x, `vfs_throughput_16k_write` 8x, `dashboard_api_health` 7x, `crypto_ed25519_verify` 7x |

The run reported as the *cleanest* of the three — `5a2002bac`, spread 2%, the
one used to certify that four earlier benchmarks had merely been contaminated —
contained a benchmark whose mean was **24x its own minimum**.

**Why the canary cannot see this.** It samples the host *between* benchmarks,
once per 8 scored entries — 10 samples across 63 benchmarks. A stall confined
to one benchmark falls between two samples and leaves no trace in it. The
canary measures the gaps; the stalls are in the benchmarks.

**Why `mean/min` can.** It is computed from the benchmark's own iterations, so
it sees precisely the interval the canary skips. And the data was already being
recorded — the append-only `mean_ns` extension landed for a different reason
(the variance band) and turns out to answer this too.

**These are not intrinsically noisy benchmarks.** That was the obvious
alternative reading, and it is wrong. Across the three runs only
`ipc_channel_sync` is *persistently* elevated (6.0 / 3.9 / 4.6). Every other
high reading is spiky — `pick_next` 15.8 then 1.1 then 1.2; `syscall_dispatch`
16.1 then 1.2 then 1.2; `page_alloc_free` 19.3, 24.4, then 1.3. A benchmark
that is 16x dispersed in one run and 1.2x in the next is being disturbed, not
behaving that way.

**Nor is it one cold first iteration.** `vfs_stat_3comp` in `f74f97b6d`: min
1334082, mean 18349532, max 758926475 over 500 iterations. The single worst
iteration accounts for only ~8% of the total time, so the elevation is broad —
many slow iterations, not one outlier. Same shape for `crypto_ed25519_verify`
(max is ~7% of total over 50 iterations).

**Mitigation applied.** `scripts/bench-history.py` now reports per-benchmark
dispersion (`suspect_dispersion` / `report_dispersion`,
`DISPERSION_SUSPECT_RATIO = 5.0`) and the canary's verdict line no longer
claims "host load stable" — it now says only that the reference access cost was
steady *between* benchmarks, and points at the dispersion line. 6 new tests
(48 total, all passing), including the real `page_alloc_free` 24x shape from the
run the canary called clean.

**The threshold is deliberately unfitted.** Measured across the three records:
median benchmark 1.26–1.59, the large majority under 2, excursions at 5–25x,
and little in between. 5.0 sits in that empty band. It wants retuning once
release-profile records exist, since optimised benchmarks run for less wall
time and so present a smaller target to a burst.

**Not yet done — this reports, it does not correct.** A flagged benchmark's
recorded figure is still its *minimum*, which may well be sound; the flag says
"do not read movement here as signal", not "this number is wrong". Deciding
which is which needs a per-benchmark dispersion *baseline*, i.e.
`TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE`, whose record count has just
been reset to zero by the debug→release profile switch.

**Lesson, the fourth of this shape.** After `TD-BENCHMARKS-...` (the suite never
ran), `B-BENCH-WATCHLIST-...` (the watch list never looked), and
`B-BENCH-COMPARATOR-...` (the diff named innocents): a canary that never fired,
read as evidence of quiet. Its own motivating case had already refuted the
first version of it, and the second version was written specifically to catch
per-benchmark bursts — yet it was still reporting "host load stable" over runs
containing 24x stalls. "It has never fired" is a claim about the check, never
about the world.
