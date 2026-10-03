### [A] TD-BENCH-RUNS-ARE-CONTAMINATED-BY-THE-AGENTS-OWN-COMMANDS. I ran greps and git during a benchmark suite after noting the machine had to be idle — 2026-08-14 — OPEN

**What.** The benchmark suite runs under QEMU TCG, which is pure emulation and
entirely CPU-bound, so any other load on the host scales the measurements.
During run 3 (`a18ea83a9`) I ran roughly a dozen `grep`, `git`, `python` and
file-read commands in the same window, despite having stated at the start of
the run that the machine needed to stay idle for the numbers to mean anything.

**Why the existing drift correction does not save it.** The median-ratio
correction removes a *uniform* whole-suite factor — a machine that is
consistently 6 % slower for the whole run. Contention from a handful of short
commands is not uniform: it lands on whichever benchmark is executing at that
moment and leaves the rest untouched. It therefore shows up as exactly what a
real regression looks like — one or two benchmarks clear of an unchanged
median. `crypto_ed25519_verify` is the canary: 30.7M → 31.4M → 158.6M ns,
i.e. two runs agreeing within 2 % and then a 5.1× jump, on a benchmark whose
source did not change and which is the longest-running in the suite (so the
most likely to overlap a command).

**Proper fix — structural, not a discipline reminder.** "Remember to stay
idle" is not a fix; it already failed once, the same day it was written down.
Make contamination *detectable* instead: have the bench harness re-run one
cheap, low-variance reference benchmark at the start and again at the end of
the suite, and record both. If the two disagree by more than a few percent,
the host load changed mid-run and the whole run should be marked contaminated
in `history.jsonl` and excluded from comparison (or at minimum reported as
such). This turns "the operator/agent must behave" into a property the data
itself can verify — the same principle as the stall detectors: a check that
cannot fire is indistinguishable from a check that passes.

**Interim mitigation until that exists:** when a `--bench` run is in flight,
do read-only work only if it is genuinely necessary, and prefer to simply
wait. Treat any single-benchmark outlier in a run that overlapped agent
activity as unproven.

**[A] ✅ FIXED 2026-08-14 — and the first version of the fix was itself blind
to the case it was built for.** Worth reading for the second half.

*Stage 1 (commit `be167dd90`).* The reference memory-access cost that already
calibrates every budget in `bench.rs` is now measured a second time at the end
of the suite, emitted as `[bench] CANARY <start> <end> <pct>`, recorded by
`bench-history.py` as a sibling key with a `contaminated` flag, and covered by
11 checks in `test-bench-history.py`. The measurement was factored into one
parameterless function used by both ends, because the comparison means nothing
unless both ends measure precisely the same thing.

*What the first real run showed (commit `be167dd90`, host Logoplex3).* Two
things, one confirming the entry and one refuting the fix.

Confirming: `crypto_ed25519_verify` came back at **30.0M ns**, against 30.7M
and 31.4M in the two runs before the spike and 158.6M during it. Three runs
now agree within 4% and the spike stands alone, so run `a18ea83a9` **was**
contaminated, exactly as this entry argued. Whole-suite drift for the new run
was −0.1%.

Refuting: the canary reported the host stable to within **3%** (283 → 275
cycles) — while in that same run `shm_rw_64bytes` (298 → 771), `tcp_checksum_v4`
(20714 → 35410), `net_ipv4_parse` (933 → 1645) and `net_ethernet_parse`
(873 → 1216) all sat 40–160% above their established values. So the run was
contaminated and the canary passed it.

*Why.* Endpoint sampling detects a **sustained** load change. The
contamination described at the top of this entry is a **transient burst** that
"lands on whichever benchmark is executing at that moment and leaves the rest
untouched" — which by construction is invisible to a check that only looks at
the two ends. The first fix was therefore a check that could not fire on its
own motivating case: the failure mode this project keeps rediscovering, arrived
at from the opposite direction.

*Stage 2 (this commit).* The reference is now sampled **throughout** the suite
— every 8th scored benchmark, giving ~8 samples across the 63 — and the verdict
uses the min-to-max spread rather than the endpoint ratio. Sampling is hooked
into `score()`, the one function every benchmark already calls, so it spreads
automatically and stays correct as benchmarks are added or reordered; a
hand-placed list of sample points in `run_all` would rot. The line gains four
append-only fields, `[bench] CANARY <start> <end> <pct> <min> <max> <spread>
<samples>`, so the single record written by stage 1 still reads back and is
still judged by the endpoint rule it was written under.

*Tolerance status.* Still 25%, still a placeholder. One clean-endpoint
observation (3%) is not a distribution, and the mid-suite spread has now to be
observed over several runs before the bound is tightened — the same discipline
applied to `TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE`. The raw min/max
are recorded on every run precisely so the bound can be retuned later against
real data instead of being invented; a stored verdict alone could never be
re-judged.

*Consequence for the four elevated benchmarks above:* unproven, not regressions.
They are diffed against `a18ea83a9`, a run this entry now shows was itself
contaminated, so the comparison is contaminated at both ends. They need a clean
run-over-clean-run comparison before anyone reads them as real.

**[A] Update 2026-08-14 — stage 2 verified, and all four elevated benchmarks
were indeed contamination.** Run `5a2002bac` reported `spread 2%` over **10**
mid-suite samples (267–275 cycles), so the sampling works end to end. Against
that clean run every one of the four returned to its established value:
`shm_rw_64bytes` 771 → **414**, `tcp_checksum_v4` 35410 → **20182**,
`net_ipv4_parse` 1645 → **952**, `net_ethernet_parse` 1216 → **829**. None was
a regression, which is what the refusal to report them was protecting.

**Honest limitation — the production check has not yet been observed firing.**
The unit tests prove the *logic* fires (a 173% mid-suite spread with quiet
endpoints reads as contaminated), and both real runs so far were clean, so the
mid-suite path has only ever been seen returning "OK". Host contamination
cannot be summoned on demand, so this is a check believed-good rather than
demonstrated-good in production — the precise distinction this entry exists to
insist on. It should not be described as proven until a real run trips it.
Whole-suite drift for `5a2002bac` was +3.1%.

**RECURRENCE 2026-08-14, run `fcd066231` — I did it again, and this time it
landed on a number I then acted on.** During the ~58 s QEMU bench run I ran
`grep` over the 60 000-line `known-issues.md`, `git log`, `git show`, and
several `Read`s. The dispersion report for that run flagged five benchmarks at
≥5x `mean/min`, and **`vfs_stat_root` was one of them at 12x**. I then took
that run's `vfs_stat_root` = 5920 ns, called it "8.5x over its 700 ns target",
committed that claim, and opened an investigation into the VFS dcache on the
strength of it.

The number may well still be broadly right — `score()` records `min_ns`, and a
burst inflates the mean far more than the min. But "broadly right" is not the
standard, and the specific escape hatch does not close here: this benchmark is
**500 iterations at ~6 µs ≈ 3 ms of wall time**. A host load episode lasting
longer than 3 ms — which is to say, essentially any of them — covers the
*entire* benchmark and inflates min and mean together, leaving `mean/min`
looking normal while every sample is uniformly slow. The 12x ratio says a
burst happened *inside* those 3 ms; it says nothing about whether a slower,
broader episode also raised the floor. So the honest status of 5920 ns is
**unverified**, not "confirmed 8.5x over".

Two things follow, and both were done rather than noted:

1. The re-measurement (the `vfs_stat_breakdown` run) is executed with **no
   agent commands issued while QEMU is running** — the read-only work is done
   before the run starts or after it finishes, never during.
2. The dcache finding is not being justified by the 5920 ns figure at all. It
   rests on reading the code: `VfsDcache::lookup` is a linear scan over 1024
   slots with a full `PathBuf` compare per slot, which is a design defect
   under CLAUDE.md's "linear scans … must be O(1) or O(log n)" rule
   independently of what any timer says. A contaminated benchmark can motivate
   a code review; it must not be the evidence.

**The pattern, stated plainly, because this is the second occurrence.** The
first time, the contamination hit numbers I merely recorded. This time it hit
a number I *reasoned from* within minutes of producing it. The entry above
correctly predicted the mechanism and even built the detector that caught it —
and the detector working did not stop me, because I read the dispersion list
*after* I had already drawn the conclusion. A check that fires after the
decision is documentation, not a gate. The ordering is the fix: read the
dispersion report **before** quoting any number from a run, not after.
