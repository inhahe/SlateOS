### [A] B-BENCH-RUN-CONTAMINATED-BY-ANOTHER-LANE-PRUNING-ITS-TARGET-DIR — 2026-08-15 — attribution recorded

**Status:** OPEN

**Why this entry exists.** The bench run at `e384f46a2` reported three
regressions. It also, by luck, is the first run where the contaminating host
activity was *identified* rather than merely suspected — so it is worth writing
down how, because the same evidence is available on every future run for free.

**The reported findings** (drift-corrected, run-over-run on this host):

```
REGRESSED (>25% slower than the suite AND outside its own recent range):
  ipc_eventfd:               537ns -> 1021ns  (+90%; own range 381-802,  median 542 over 8 runs)
  http_build_response_1KiB: 8546ns -> 12431ns (+45%; own range 4810-7932, median 6004 over 8 runs)
  vfs_stat_root:            3278ns -> 4488ns  (+37%; own range 2881-4290, median 3613 over 8 runs)
```

**The contamination, measured.** Free space on `D:` was sampled three times
across the run: **41 GB → 45 GB → 49 GB**. Something was deleting on the same
volume throughout. Per-directory sizing identified it: the **integration
checkout's** build output fell **59.1 GB → 51.5 GB** — 7.6 GB of files deleted
concurrently with the benchmark. That is tens of thousands of filesystem
metadata operations on the volume backing the QEMU image, during a run whose
three findings are all I/O-adjacent (a VFS stat, an HTTP response build, an
eventfd round-trip).

**The run graded itself UNPROVEN independently of any of this:**

```
RUN UNPROVEN: nothing fired, but not every instrument could measure
  - dispersion: 2 benchmark(s) stalled (ipc_channel mean 31x its min; tcp_checksum_v4 7x)
  - canary: steady, but CANNOT see host descheduling at all
```

So the harness and the external evidence agree, which is the useful part: the
UNPROVEN verdict was not over-cautious boilerplate — on this run there really
was a specific, nameable disturbance, and the harness flagged it without being
able to see it directly.

**What this does NOT license.** It is tempting to now dismiss the three
regressions as contamination and move on. That is the same reasoning error in
the other direction: "there was noise on this run" is not "these three numbers
are noise." `ipc_eventfd` at +90% is well outside its 8-run range and deserves a
verdict, not an excuse. The disposition is a re-run with host load sampled
(`build/hostload.tsv`, 10 s cadence), not a dismissal.

**Free technique for future runs.** Sampling free space on the volume during a
benchmark costs nothing and detects the single most common contaminant here
(another lane building or cleaning). It catches what the in-guest canary
provably cannot: the canary counts *guest* cycles, which do not advance while
the host is busy elsewhere, so it reads its cleanest possible verdict on exactly
the runs that are most disturbed.
