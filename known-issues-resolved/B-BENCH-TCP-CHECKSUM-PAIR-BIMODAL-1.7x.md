### [A] B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x. A hot loop that straddles a 4 KiB guest page costs ~1.7x under TCG, deterministically per build — ROOT-CAUSED 2026-08-14, fix pending

**Where:** `kernel/src/bench.rs`, `bench_net_tcp_checksum_v4` (3281) /
`bench_net_tcp_checksum_v6` (3340) and their bench-local kernels
`tcp_checksum_bench` (3309) / `tcp_checksum_v6_bench` (3366).

**What.** In 3 of the 5 recorded runs on host `Logoplex3`, one member of the
pair sits near ~35000 ns while the other sits near ~20000 ns; in the other 2
runs both sit in the 20000–26000 band. Which member is the elevated one
varies:

| commit | `tcp_checksum_v4` | `tcp_checksum_v6` |
|---|---|---|
| `bf26aabdb` | 20667 | 23021 |
| `17dbde179` | 25279 | 25751 |
| `a18ea83a9` | 20714 | **35899** |
| `be167dd90` | **35410** | 20953 |
| `5a2002bac` | 20182 | **35039** |

The two kernels are near-identical byte-at-a-time fold loops over the same
1460-byte segment; v6 does 36 more pseudo-header bytes than v4, i.e. ~2.5%
more work. A 1.7x gap between them — in either direction — is not explicable
by the work they do.

**What the dispersion data does and does not show.** The recorded figure is
`result.min_ns`, the **minimum** over 2000 iterations, and since the
append-only `mean_ns` extension landed we also record the mean, so `mean/min`
is available as a within-run dispersion measure:

| commit | benchmark | min | mean/min |
|---|---|---|---|
| `be167dd90` | `tcp_checksum_v4` (elevated) | 35410 | 1.16 |
| `be167dd90` | `tcp_checksum_v6` | 20953 | 1.20 |
| `5a2002bac` | `tcp_checksum_v4` | 20182 | 1.21 |
| `5a2002bac` | `tcp_checksum_v6` (elevated) | 35039 | 1.33 |

In both runs the elevated member's dispersion is indistinguishable from the
other member's. Compare the visibly burst-hit numbers in the same records:
`net_ethernet_parse` at 2.86 and `context_switch` at 10.62 in `be167dd90`.
So the elevated member is uniformly ~1.7x slower across all 2000 iterations
with normal spread.

**This rules out a sub-benchmark burst, and nothing more — do not read it as
"not contamination".** A first draft of this entry concluded that a normal
`mean/min` proved the slowdown was a steady-state property of the build. That
does not follow. 2000 iterations at ~20 µs is only ~40 ms of wall time, and a
host load episode that spans the *entire* 40 ms window inflates the min and
the mean by the same factor, leaving `mean/min` untouched. Such an episode is
entirely ordinary on a desktop. So the dispersion data distinguishes "a spike
during part of the benchmark" from "uniformly slower", and is silent on
*why* it was uniformly slower. Both a build property and a benchmark-length
contamination episode predict exactly what is in the table above.

**Two live hypotheses, and the test that separates them.**

1. *Code-layout sensitivity under QEMU TCG.* The two loops are compiled
   separately (deliberately duplicated "to avoid depending on tcp module
   internals"), so their alignment and translation-block boundaries shift with
   every unrelated code change; whichever lands unluckily pays a fixed
   per-iteration penalty.
2. *A contamination episode long enough to cover one whole benchmark.* The
   mid-suite canary samples every 8 scored benchmarks, so an episode lasting
   one benchmark can slip between two samples and be reported as a quiet run —
   which is what `5a2002bac` reported.

**These are separated by re-running the bench on the *same commit*.** Hypothesis
1 is a property of the binary and must reproduce: same member elevated, same
factor. Hypothesis 2 re-rolls: the elevated member moves, or neither is
elevated. This needs no new code, just a second `--bench` boot on an unchanged
tree.

**RESOLVED 2026-08-14 — hypothesis 1, decisively.** That run was done on a
byte-identical binary (only markdown had changed since `5a2002bac`):

| | `5a2002bac` | re-run, same binary | agreement |
|---|---|---|---|
| `tcp_checksum_v4` | 20182 | 20687 | 2.5% |
| `tcp_checksum_v6` | **35039** | **35048** | **0.03%** |

The same member is elevated, at the same value — and the host was *noisier*
this run, not quieter (canary spread 16% over 10 samples, against 2% before),
which rules out the contamination reading rather than merely failing to
support it. It is a deterministic property of the binary.

**Mechanism: the elevated member's hot loop straddles a 4 KiB guest page.**
Disassembling the staged binary and locating the backward branch in each fold
loop:

| | fold loop | span | pages |
|---|---|---|---|
| `tcp_checksum_bench` (v4, fast) | `ffffffff805d7202` → `ffffffff805d73f7` | 501 B | `…805d7` → `…805d7`, **one page** |
| `tcp_checksum_v6_bench` (v6, elevated) | `ffffffff805d9ea9` → `ffffffff805da086` | 477 B | `…805d9` → `…805da`, **straddles** |

Under TCG a translation block is bounded by the guest page — a loop that
crosses a page boundary cannot stay a single directly-chained TB, so every
iteration pays a dispatcher round-trip instead of a direct jump. That predicts
exactly what is observed: a *uniform* per-iteration penalty (so `mean/min` is
untouched), perfectly reproducible on the same binary, and re-rolled whenever
unrelated code shifts the function's address — which is why runs 1 and 2 show
neither member elevated (in those builds neither loop straddled).

**Falsifiable prediction, to be checked on the next bench run:** disassemble
first, and whichever of the two fold loops straddles a page is the one that
will come back elevated — with neither elevated if neither straddles. This
entry should be treated as provisional until that prediction has been made
*before* a run and held.

**This generalises to the whole suite, and that is the real finding.** Nothing
about the mechanism is specific to `tcp_checksum`. Any benchmark whose hot loop
happens to straddle a 4 KiB page pays the same penalty, and which benchmarks do
re-rolls at every build. So commit-to-commit comparison under TCG carries an
irreducible per-benchmark noise floor of up to ~1.7x that is *deterministic
within a run* — meaning neither the canary nor `mean/min` can ever detect it,
because both look for variation and there is none. Every noise-suppression
mechanism built for this suite so far is structurally blind to it.

**It is also mostly the same bug as
`B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL`, and mostly the same
fix.** The straddle probability scales with the byte length of the hot loop. At
`opt-level = 0` this fold loop is 117 instructions / ~500 bytes, giving it
roughly a 1-in-8 chance of crossing any given page; optimised it would be a
few dozen bytes, closer to 1-in-100. Building the bench kernel `--release`
therefore shrinks this noise source by about an order of magnitude as a side
effect. Do that first and re-measure before considering anything more invasive
(forced function alignment via `-Z align-functions` costs padding across the
whole kernel and would only paper over the loop-length problem).

**Why it matters.** Both are on the `over_target` list (targets 2000/2200 ns,
measured 20000–35000), so the absolute numbers are already known-bad under TCG
and nobody is being misled about pass/fail. The damage is to the *comparator*:
a 1.7x swing that re-rolls every build is pure noise in any commit-to-commit
diff, and `TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE` will size its
band from exactly this history. If the band is fitted without knowing this
pair is bimodal, it will either be stretched wide enough to hide real
regressions everywhere else, or it will keep flagging these two forever.

**Remaining plan.** Steps 1 and 2 are done (above). What is left:

1. Build the bench kernel `--release` (see
   `B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL`) and re-measure.
2. Make the straddle prediction *before* that run and record it, so the
   mechanism is confirmed prospectively rather than fitted after the fact.
3. If page straddling still moves benchmarks materially at `opt-level = 3`,
   teach the comparator about it: the check is mechanical (disassemble, locate
   the backward branch, compare `addr >> 12` at both ends) and could be emitted
   alongside each score, which would turn an invisible deterministic bias into
   a recorded per-benchmark flag.

**Reproducing the disassembly.** `llvm-nm` / `llvm-objdump` ship with the
rustup toolchain — no binutils install needed:
`~/.rustup/toolchains/stable-x86_64-pc-windows-gnu/lib/rustlib/x86_64-pc-windows-gnu/bin/`.
The two kernels are `_ZN6kernel5bench18tcp_checksum_bench…` at
`ffffffff805d7130` and `_ZN6kernel5bench21tcp_checksum_v6_bench…` at
`ffffffff805d9df0`. Note the symbol hash differs per build, so match on the
demangled prefix rather than pasting a mangled name.

**Incidental finding from the disassembly: the benchmarked kernel is built
without optimisation.** `tcp_checksum_bench` spills every intermediate to the
stack (`movl %eax, -0x64(%rbp)` after each add). That is consistent with the
whole suite sitting ~10x over targets that were set from optimised reference
implementations, and it means the absolute numbers measure debug codegen under
TCG, not the code that would ship. Worth confirming against the boot-test
build flags and recording separately — it is a much larger effect than the
1.7x this entry is about, and it is not this entry's subject.

**Related observation — `mean/min` sees contamination the canary missed.** The
canary called run `5a2002bac` clean (spread 2% over 10 samples). In that same
run `crypto_ed25519_verify` had mean 323487129 against min 31875588, a
**10.15x** ratio; `context_switch` had 10.62x in the run before it. The canary
samples the host *between* benchmarks; `mean/min` measures dispersion *inside*
the benchmark that was running, so it catches a burst that fell between two
canary samples. The data is already recorded per benchmark and needs no
cross-record history to interpret, so a per-benchmark "this number is suspect"
flag is implementable now.

Neither measure dominates the other, and the reason is exactly the failure
above: `mean/min` is blind to any slowdown that covers a whole benchmark
uniformly — including a sustained load change, which is what the canary
endpoints exist to catch — while the canary is blind to bursts shorter than
its sampling interval. The comparator should consult both, and should treat
"canary quiet **and** `mean/min` normal" as the only combination that
licenses reading a number as real.

**PROSPECTIVE PREDICTION, recorded 2026-08-14 BEFORE the first release-profile
bench run.** This entry says above that the page-straddle mechanism "should be
treated as provisional until that prediction has been made *before* a run and
held." This section is that prediction. It was written from the disassembly of
`target/x86_64-unknown-none/release/kernel` (built clean, 0 warnings, 9m25s)
with **no release-profile measurement in existence yet** — the first such run
has not been performed. Whatever the numbers turn out to be, this text is not
to be edited afterwards; the result goes in a separate section below it.

Structural facts read out of the release binary:

| | v4 | v6 |
|---|---|---|
| closure inlined into the timed loop? | **yes** | **no** — `callq`+`ret` per iteration |
| hot fold loop | `ffffffff80985cc2`–`…985cf7` | `ffffffff80976ba0`–`…976bc7` |
| straddles a 4 KiB page? | **no** (all in `…985000`) | **no** (all in `…976000`) |
| timed outer loop | `…985caa`–`…985d51` (page `…985`) | `…9864a5`–`…9864fd` (page `…986`) |
| per-iteration indirect branch | none | one `ret` |
| bytes consumed per loop iteration | 4 (2x unrolled) | 4 (2x unrolled) |

So in the release build the *specific* mechanism this entry root-caused — a
hot loop split across a guest page boundary — is **not active for either
benchmark**. Both fold loops are comfortably interior to a page. If the 1.7x
bimodal swing were caused by anything else, it should survive the profile
change; if it was the straddle, it should vanish.

Predictions, in falsifiable form:

1. **The 1.7x v6/v4 gap collapses.** Predicted release ratio **1.00–1.20**.
   A ratio still ≥1.5 falsifies the straddle explanation outright.
2. **A residual v6 penalty is still expected, but small.** v6 pays one
   out-of-line call and — the part that actually costs under TCG — one `ret`,
   which is an *indirect* branch and cannot be direct-chained between
   translation blocks; it takes a jump-cache lookup every iteration. But that
   is one dispatch amortised over ~365 fold-loop iterations of real work, so
   it should be a low-single-digit percentage, not a multiple. v6 also has the
   genuinely larger 40-byte pseudo-header (the straight-line preamble at
   `…976aa3`–`…976b8e`), which is real work and legitimately makes v6 slower.
3. **Both numbers drop by roughly an order of magnitude** from the debug
   figures (v4 ~20200–20700 ns, v6 ~35000 ns). The debug loop spilled every
   intermediate to the stack and consumed 2 bytes per iteration; the release
   loop is 10 instructions, register-only, 4 bytes per iteration. Predicted
   release: **v4 ~2000–3000 ns, v6 ~2200–3500 ns** — i.e. at or near the
   2000/2200 ns targets, which were set from optimised reference
   implementations and have been failed by ~10x for the whole life of the
   suite for exactly that reason.
4. **The run is scored against no baseline.** `bench-history.py --profile
   release` should report that no same-profile record exists and decline to
   diff against the five debug records, rather than reporting a fabricated
   ~10x "improvement". This is the profile-isolation change under test.

If (1) holds and (3) holds, the mechanism is confirmed prospectively and the
entry can be closed. If (1) fails while (3) holds, the optimisation level was
a confound and the straddle explanation is wrong — in that case the same-binary
re-run table above (v6 35048 vs 35039, 0.03%) still stands as proof the effect
is deterministic per build, and a different per-build mechanism must be found.

**RESULT of the prediction above — run `fcd066231`, release profile,
2026-08-14T15:57:59.** Scored against the four predictions as written, with no
edits to them:

| | Predicted | Measured | Verdict |
|---|---|---|---|
| 1. v6/v4 ratio | 1.00–1.20 (≥1.5 falsifies) | **0.93** | central claim **holds**, band missed |
| 2. v6 slightly slower than v4 | yes, low single-digit % | v6 **6.6% faster** | **WRONG** |
| 3. both drop ~10x | v4 2000–3000 ns, v6 2200–3500 ns | v4 **1716**, v6 **1602** | order right, **both beat the band** |
| 4. no cross-profile baseline diff | refuses to compare | refused, verbatim | **holds exactly** |

Raw: `v4 min 1716 ns (6366 cyc), mean 1772` and `v6 min 1602 ns (5946 cyc),
mean 1663`. Dispersion 1.03 and 1.04 — both clean, so neither number is a
contaminated read. Against the debug records (v4 20182–35410, v6 20953–35899)
that is **11.8x and 21.9x faster**, and both now pass their 2000/2200 ns
targets — the first time either has, ever.

The bimodality is gone outright. Across the six debug records the ~35000 band
was occupied by v6, v6, v4, v6, v6 and neither (a middle run at 25279/25751);
in release both members sit in a 1602–1716 band with no elevated member. So
the entry's *central* claim is confirmed: **the 1.7x swing was an artefact of
the build, not a property of the checksum code.**

**But this run does not isolate the page-straddle mechanism, and it would be
dishonest to close the entry as if it had.** Going from `opt-level = 0` to `3`
rewrote the code completely — new instruction sequences, 2x unrolling, new
addresses, new inlining decisions. The straddle hypothesis predicted the gap
would vanish and it vanished; but so would *any* hypothesis of the form "this
is a build artefact", which is a much weaker and much easier claim. I changed
two variables at once and can only credit the one they share. The experiment
confirms the **class**, not the **mechanism**.

**Prediction 2 failing matters more than prediction 1 succeeding.** v6 does
strictly more work than v4 — a 40-byte pseudo-header instead of 12 — *and* in
this build pays an out-of-line `callq` plus a `ret` (an indirect branch, not
direct-chainable between TCG translation blocks) on every one of its 2000
iterations. It came out faster anyway. That is the same fine-grained "what
costs what under TCG" reasoning the straddle story rests on, applied to a case
where the answer was checkable, and it got the *sign* wrong. Confidence in the
straddle attribution should be downgraded accordingly, not raised by
prediction 1.

**The experiment that would actually isolate it** (not yet done): stay within
one profile and move a function's address deliberately — insert padding, or a
`#[repr(align)]`/`.balign` on the hot loop — so that a loop which currently
sits interior to a page is pushed across a boundary, with nothing else
changed. Same optimisation level, same instructions, same trip count, one
variable. Until that is run, "TCG translation blocks are page-bounded" remains
a plausible and well-documented QEMU property that *fits* the data rather than
a mechanism this project has demonstrated.

**Much larger incidental result: the profile switch moved the whole suite.**
`over_target` went **58–59 of 63 on every debug record to 15 of 63 on
release** — scorecard `48/63 within hardware target`. The suite had been
reporting a near-total failure that was overwhelmingly an artefact of
measuring unoptimised codegen, exactly as
`B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL` predicted. The 15
remaining over-target entries (`syscall_dispatch` 661 ns vs 200,
`futex_wake_empty` 944 vs 500, `futex_wait_mismatch` 1507 vs 500,
`vfs_stat_root` 5920 vs 700, `vfs_stat_deep_2comp` 31046 vs 1400,
`isr_latency` 164652 cyc vs 37000, …) are now the first *credible* performance
findings this suite has produced, because they are the first measured on the
code that would ship. They should be triaged on their own merits — `vfs_stat`
at 22x and 8x target is the standout — and are not this entry's subject.

**Caveat on those 15, added after the fact.** This run's dispersion report
flagged five benchmarks at ≥5x `mean/min`, and `vfs_stat_root` — the one
singled out as "the standout" above — was among them at **12x**. I ran greps
and git commands during the QEMU boot, which is exactly the mistake recorded
in `TD-BENCH-RUNS-ARE-CONTAMINATED-BY-THE-AGENTS-OWN-COMMANDS`. The
over-target *set* is unlikely to be an artefact (a 22x miss does not come from
host noise), but the individual magnitudes from this run should be treated as
provisional until re-measured on an idle host. See the RECURRENCE note in that
entry for why `min_ns` does not fully rescue a 3 ms benchmark.
