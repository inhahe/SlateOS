## A-BENCH-PAGE-ALLOC-ZEROED-FREE-IS-AN-UNSTABLE-MEASUREMENT, NOT A REGRESSION (lane A, 2026-08-18) - **open; my own earlier headline here was wrong, see the correction at the end**

**In short:** one benchmark, `page_alloc_zeroed_free`, sometimes reports a
number about 40-80% higher than usual.  It looked like a slowdown that had
"left its historical range", and this entry originally said so.  **That was
wrong** - reading all 29 runs instead of the last few shows the same unchanged
binary producing both the normal and the elevated number, so the benchmark is
an unreliable ruler rather than a record of code getting slower.  Nothing needs
to be fixed in the allocator; the *measurement* needs fixing.  **Read the
CORRECTION section at the end of this entry before using anything above it** -
the original numbers below are left in place because the correction only makes
sense next to the claim it corrects.

### The numbers (as originally recorded - see the CORRECTION below)

| run | value | own recent range | median over 8 runs |
|---|---:|---|---:|
| baseline (commit `61a4998c1`) | 3 680 ns | 2 909-4 717 ns | 3 645 ns |
| run 1 (`e2f2a2726`) | 5 125 ns | " | " |
| run 2 (**identical binary** to run 1) | 6 652 ns | " | " |

Flagged `+35%` then `+27%` against the suite, i.e. after drift correction, and
run-over-run drift itself was small both times (+2.8%, +2.1%).

### Why it is *not* being attributed to the FpuState / KernelFdTable boxing

The tempting story is that boxing `FpuState` (4 KiB) and `KernelFdTable`
(8 KiB) added two heap allocations per task creation, changing kernel-heap
layout and therefore page-allocator free-list state.  That is mechanically
plausible.  It is also not what the data shows:

- **The second step happened with an unchanged binary.**  Run 2 is the same
  bytes as run 1 and still rose 27%.  Whatever caused that step is not code.
- **A code change produces a step, not a ramp.**  3 680 -> 5 125 -> 6 652 is a
  ramp across three runs, two of which share a binary.
- **This benchmark is a known high-variance one.**  Run 1's dispersion report
  lists it explicitly: `page_alloc_zeroed_free: mean is 26x its min`.  A
  benchmark whose mean is 26x its minimum will be flagged often, by
  construction.

So the honest reading is: at least the second step is environmental, which
removes the grounds for reading the first step as code.  What remains true and
worth tracking is that the current value sits 41% above the top of its own
eight-run range.

### What would settle it

Run the suite on the commit *before* the boxing changes (`61a4998c1`) and on
`HEAD`, alternating, three runs each, and compare medians rather than single
runs.  Alternating matters: consecutive runs share whatever the host was doing,
which is exactly the confound above.  Until that is done this stays open and
unattributed - and specifically must **not** be quoted as evidence that the
boxing changes cost anything, because it is not.


### CORRECTION 2026-08-17 - the headline above was overstated

I wrote the heading "has left its historical range" from three consecutive
runs. Reading the *entire* release-profile series (n = 29) instead of the tail
does not support it. The number the scorecard reports and that drives verdicts
is the `entries` field; `mean_ns` is a separate, far noisier statistic.

**`entries`, every release run, oldest to newest:**

```
3631 3626 3502 3473 3687 3585 3647 5114 3558 3609 3659 3595 3734 3636 3643
3645 3642 3533 3557 5230 3570 5117 3553 3593 3632 3658 3680 5125 6652
```

n=29, min 3473, median 3636, max 6652.

**The three claims in the original entry, checked:**

| Claim | Verdict |
|---|---|
| 5125 is outside the historical range | **False.** 5114 (`d542299e2`) and 5230 / 5117 (`f61bc4e71`) all predate the boxing commits. |
| The series ramps 3680 -> 5125 -> 6652 | **True but meaningless** - see the same-binary spreads below. |
| 6652 is a new maximum | **True.** It is the only genuinely new value, and it is one sample. |

**The finding that actually matters - identical binaries land in both modes:**

| Commit | `entries` across repeat runs | spread |
|---|---|---|
| `f61bc4e71` | 5230, 5117, 3593, 3658 | **1.5x** |
| `d542299e2` | 5114, 3558 | **1.4x** |
| `53cb74578` | 3557, 3570, 3553, 3632 | 1.0x |
| `602fc62e0` | 3502, 3473 | 1.0x |

`f61bc4e71` produced both 5230 and 3593 from **the same binary**. A benchmark
that swings 1.5x with the code held constant cannot support a 1.4x per-commit
attribution, which is precisely what the original heading was doing. The right
description is an unstable measurement with an occasional elevated mode, not a
regression that needs a culprit.

**`mean_ns` has two distinct high populations**, which is worth recording
because it shows the elevated `entries` and the huge means are not one
phenomenon:

| Population | `mean_ns` | `entries` | Runs |
|---|---|---|---|
| A | ~65k-68k | **normal** (3502-3734) | `602fc62e0`, `9ecef3188`, `e3ae7bae1` |
| B | ~131k-135k | **elevated** (5114-6652) | `d542299e2`, `37d1a4bb1`, `e5a6b2183` |

So a run can have a 17x mean with a perfectly ordinary reported figure, and
`mean_ns` should be treated as diagnostic only.

### The reported number is ALREADY a minimum - which makes this worse, not better

I first wrote here that the fix was "report a minimum or median rather than a
mean". **That was wrong, and checking `bench.rs` rather than assuming is what
caught it.** `record()` publishes

```rust
measured_ns: result.min_ns,
```

so the `entries` figure this whole entry is about is the **minimum over 500
iterations** already. There is no mean-to-min fix available; it has been the
min all along.

That inverts the conclusion. A *mean* swinging 1.5x is unremarkable - a handful
of multi-millisecond stalls will do it. A **minimum over 500 iterations**
swinging 1.5x on an unchanged binary is a much stronger claim: for the floor to
rise 50%, essentially *every one* of the 500 iterations had to get slower. That
is not occasional interference, it is a sustained condition lasting the entire
measurement window.

And `bench.rs` says exactly what that condition is, in the comment right above
the benchmark:

> Tracked, not scored: this is the cold path, whose cost is dominated by a
> 16 KiB memset and therefore by host memory bandwidth, so a published
> per-allocation figure is not the right yardstick.

So `page_alloc_zeroed_free` is, by its own author's description, largely a
**host memory-bandwidth gauge**. When something else on this desktop is moving
memory for a sustained period, the floor for a memset-bound loop genuinely
rises, for every iteration, and the min moves with it. The benchmark is
behaving correctly; it is measuring the host, and the host is a workstation.

### Does the existing SplitCheck already catch it? Weakly - below its threshold

`bench.rs` already has machinery for precisely "the achievable floor moved
during the window": `SplitCheck` compares the min over the first half of the
iterations against the second half, and `min_cycles` is not a stable property
of the code when they diverge. Checking it against the five elevated runs:

| `entries` | split | |
|---:|---:|---|
| 5114 | 3 | elevated |
| 5230 | 2 | elevated |
| 5117 | 9 | elevated |
| 5125 | 6 | elevated |
| 6652 | 0 | elevated |

Non-zero split on **4 of 5** elevated runs, against **4 of 24** normal runs -
an association, but at magnitudes (2-9%) far below the level that prints a `!`.
Meanwhile the two runs that *were* loudly flagged (`44!` and `57!`) had entirely
ordinary values of 3473 and 3680.

So the split check carries some signal here and is not firing on it, while
firing on runs that were fine. With n=5 that is a lead, not a conclusion, and
it should not be "fixed" by lowering a threshold until there is more data -
the flagged-but-fine cases say a lower threshold would mostly add false alarms.

### The orphaned QEMU spinner - a real factor, but NOT the explanation

`A-ADHOC-QEMU-PROBES-LEAK-THE-EMULATOR-ABOUT-TWO-THIRDS-OF-THE-TIME` records a
leaked QEMU that burned 729 s of CPU on this host. Checking the clock rather
than assuming (history timestamps are **UTC**; the host runs EDT = UTC-4):

* Spinner alive: 2026-08-17 01:04 EDT until killed ~22:02 EDT (~21 h).
* `37d1a4bb1` ran 2026-08-18T00:49Z = 08-17 **20:49 EDT** - spinner alive.
* `e5a6b2183` ran 2026-08-18T01:13Z = 08-17 **21:13 EDT** - spinner alive.

Tempting, and wrong to stop there. Two facts kill it as *the* cause:

* `d542299e2` (entries 5114) ran 08-16 22:49 EDT, **before the spinner existed**.
* `f61bc4e71` produced its two *low* readings (3593, 3658) at 15:43 / 15:50 EDT,
    with the spinner alive the whole time.

So the spinner was present for both elevated and normal readings, and one
elevated reading predates it entirely. It is a genuine contaminant that should
never have been running, but it does not explain this benchmark's bimodality.

### Falsifiable prediction, recorded before the run that tests it

The `--bench` run started 2026-08-17 22:10 EDT for the SCHED lock-order hoists
is the first benchmark run on this host since the spinner was killed.

* **If the spinner mattered**, `page_alloc_zeroed_free` returns to the ~3600
    cluster.
* **If it does not**, the value is a coin-flip over the historical
    distribution - roughly a 5/29 chance of landing >= 5000 regardless.

Either way **one run decides nothing**, which is the whole point of
`A-BENCH-THE-HOST-IS-A-DESKTOP-SO-A-SINGLE-RUN-IS-NEVER-A-VERDICT`. Recorded in
advance so the result cannot be read backwards into whichever story fits.

### Outcome of that prediction - run 86, `e80fe679f`, 2026-08-17 22:39 EDT

`page_alloc_zeroed_free` = **3853 ns**: the low cluster, as the
spinner-mattered branch predicted.

**This is very nearly zero evidence, and saying so is the point of having
written the prediction down first.** The prediction was a bad one - not wrong,
*uninformative* - because both branches predicted the same outcome with almost
the same probability:

| Hypothesis | P(low cluster) | Observed |
|---|---|---|
| the spinner was the cause | ~1.0 | low |
| the value is drawn from the historical distribution | 24/29 = 0.83 | low |

The likelihood ratio is about 1.2:1. A prediction whose two branches differ by
20% cannot separate them in one trial; only the >= 5000 outcome would have said
anything, and that outcome was unlikely under *either* hypothesis. Recording it
in advance stopped it being read backwards, which is worth something, but the
experiment as designed could not have paid out.

Two further reasons not to lean on this reading at all:

* 3853 is the **highest low-cluster value on record** (the previous low-cluster
    maximum was 3734 at `e3ae7bae1`). If anything it drifts toward the gap, not
    away from it.
* Run 86 is itself flagged `RUN CONTAMINATED` - the reference-access canary
    spread 28% over 13 samples against a 25% tolerance, 16 benchmarks stalled,
    and `page_alloc_zeroed_free`'s own mean was **29x its min**. A contaminated
    run is not the instrument you settle a bimodality question with.

The entry's conclusion is unchanged: this benchmark is an unstable ruler, it is
tracked-not-scored for that reason, and the only experiment that would settle
it is the alternating-runs design at the end of this entry - not another single
observation.

### Run 87 settles it: the same binary, twice, 3853 -> 5659

Run 86 flagged `sched_pick_next_d8` as `REGRESSED, UNREPLICATED` and the report
prescribed its own remedy - re-run `--bench` *without rebuilding*.  That was
done immediately (run 87, 2026-08-17 23:0x EDT).  `cargo` reported
`Finished release profile in 6.00s` with no compilation and the ELF's mtime
stayed at 22:36:38, so runs 86 and 87 measured a **byte-identical kernel**.

| benchmark | run 86 | run 87 | verdict |
|---|---:|---:|---|
| `sched_pick_next_d8` | 45 ns | **38 ns** | contradicted; inside its 34-44 range |
| `page_alloc_zeroed_free` | 3853 ns | **5659 ns** | +47% on an unchanged binary |

Two conclusions, one for each row.

**The SCHED lock-order hoists did not regress `sched_pick_next_d8`.** The 45 ns
reading was environmental: the whole `sched_pick_next_*` family sat at 44-45 ns
in run 86 (d1, d8, d64, d256, d1024 all within 1 ns of each other, against
per-benchmark medians of 38-42 ns), which is the signature of a suite-wide floor
shift, not of a change that would have to be depth-dependent to be real.  Run 86
was flagged `RUN CONTAMINATED` and run 87 put the family back at 38 ns.

**The `page_alloc_zeroed_free` bimodality is environmental, and is now proven so
without needing any of the earlier reasoning.** This is the alternating-runs
experiment's first pair, and it came back as clean a result as that design could
produce:

* Same binary. No rebuild, no code change, no commit between the two readings.
* Back to back, ~20 minutes apart.
* **The orphaned QEMU spinner was dead for both.** So the elevated mode occurs
    with the spinner gone - which retires the spinner hypothesis outright,
    rather than merely failing to support it as the run-86 reading did.
* The elevated reading is the *second* of the pair, so it cannot be dismissed as
    a warm-up artefact of a cold host.

That is a 1.47x swing in a **minimum over 500 iterations** with the code held
fixed.  Nothing about a commit can be read off this benchmark, and the
`In short:` headline of this entry - an unstable measurement, not a regression -
is now supported by direct replication rather than by inference from the series.

**The prediction two sections up is therefore moot** and should not be cited:
its low-cluster outcome was uninformative at 1.2:1, and this pair supersedes it
with a controlled comparison.

### What to do

* **Do not cite this benchmark as evidence for or against any commit**,
    including the `FpuState` / `KernelFdTable` boxing. It cannot carry that
    weight.
* **Do not "fix" it by changing the estimator** - it is already a minimum over
    500 iterations. The earlier version of this entry recommended exactly that,
    from an assumption about the code rather than a reading of it.
* The honest options are (a) leave it tracked-but-unscored, which is what it
    already is and what its own comment argues for, or (b) normalise it against
    a deliberate host-memory-bandwidth reference measured in the same boot, so
    the number reports the allocator rather than whatever else the desktop was
    doing. (b) is the only one that would make it diffable boot-over-boot.
* If someone does want to settle whether the elevated mode is real, the
    experiment is alternating runs of two commits, three each, comparing
    medians - not consecutive runs of successive commits.
