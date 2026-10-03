## A-BENCH-THE-HOST-IS-A-DESKTOP-SO-A-SINGLE-RUN-IS-NEVER-A-VERDICT (lane A, 2026-08-18) - **not a bug; methodology, recorded so it stops being rediscovered**

**In short:** the benchmark suite flags "regressions" that are not real, on
roughly half of all runs.  This is not a fault in the suite or in the
contamination canary - both are working.  It is that the machine running the
benchmarks is somebody's desktop, with Unreal Editor, Chrome, Creative Cloud,
the Epic Games launcher and assorted node processes resident, and it is never
going to be quiet.  The only thing that separates a code regression from
ambient noise on this host is **replication**, and this entry records the
measurement that proves it, so the next session does not spend an hour
re-deriving it.

### The measurement

Two `--bench` runs of the **byte-identical kernel binary**, back to back, with
nothing started by the agent during either QEMU window:

| | run 1 (2026-08-18T00:49) | run 2 (replication) |
|---|---|---|
| benchmarks flagged `REGRESSED, UNREPLICATED` | 9 | 12 |
| whole-suite vs 8-run median | x1.216 | x1.291 |
| canary verdict | contaminated (spread 76%) | contaminated (spread 164%) |

**Overlap between the two sets: 1 benchmark out of 20. Jaccard 0.05.**

Run 1 flagged `net_arp_lookup`, `vfs_stat_deep`, `crypto_ed25519_sign`,
`crypto_x25519`, `service_connect`, `page_alloc_zeroed_free`,
`cp_try_wait_empty`, `crypto_sha256_64B`, `ipc_channel_sync`.
Run 2 flagged `http_gzip_1KiB`, `http_build_response_1KiB`,
`vfs_stat_breakdown_prologue`, `vfs_write_256`, `dashboard_api_status`,
`ipc_channel_roundtrip_64k`, `vfs_stat_3comp`, `crypto_sha256_1KiB`,
`page_alloc_zeroed_free`, `ipc_channel`, `vfs_stat_breakdown_ns`, `ipc_pipe`.

Same code.  Nineteen of the twenty are noise.

### Three wrong hypotheses, recorded because each was plausible

Getting to that answer took three attempts, and the two failures are worth
keeping because both are the sort of thing that sounds right:

1. **"A single unlucky sample condemns the run."**  Wrong.  `ab_interleaved`
   already takes the **minimum over 500 interleaved rounds** per arm, which is
   a sound estimator against an interrupt landing in one measurement.  The
   sampling was never the weak part.
2. **"The canary is reading the suite's own TLB residue, not the host."**
   This one had real evidence behind it - run 1's `CANARY-TRACE` showed the
   elevation confined to two *adjacent* positions (48:10.6, 56:9.4) with
   everything else flat at 6.0-6.6, which is not what ambient load looks like,
   and the two endpoints agreed to 2% (`start=6, end=6, pct=102`), ruling out
   drift.  It predicted the bump would recur at the same positions on an
   identical binary.  It did not: run 2's bump was at 40 (11.6) and 64 (16.5).
   Refuted by its own prediction, which is the good outcome.
3. **"The host is busy."**  Correct.  Cumulative CPU on the box is dominated by
   `UnrealEditor`, `explorer`, `Creative Cloud`, `EpicGamesLauncher`, `chrome`
   and `node`.  Every instrument was reporting this accurately the whole time.

The general lesson is the one this project keeps relearning from the other end:
*before concluding an instrument is lying, check whether the thing it is
measuring is actually true.*  Two sessions have now gone looking for a defect
in this canary that was not there.

### What to actually do

- **Never accept or reject a benchmark movement from one run.**  The harness
  already says this (`REGRESSED, UNREPLICATED` -> "re-run WITHOUT rebuilding to
  confirm").  Follow it literally: `git stash` any uncommitted source changes
  first, so `cargo` has nothing to rebuild and the second run is provably the
  same binary.
- **A `RUN CONTAMINATED` verdict does not invalidate the boot test.**  Both runs
  above passed every correctness gate.  Contamination taints the *numbers*, not
  the *pass*.
- **Do not chase a whole-suite outlier.**  `!! OUTLIER RUN: everything measured
  slower than usual by N%` means the comparison denominator is bad; drift
  correction removes the uniform part but cannot remove a non-uniform one.
- The one thing that *would* be worth building: a mode that runs the suite
  **twice in one boot** and reports only benchmarks that moved in both halves.
  That gets replication without paying a second 17-minute release build, and it
  is the only change here that would improve the signal rather than just
  documenting the noise.  Not built yet.
