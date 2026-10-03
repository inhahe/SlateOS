## `[A]` The bench gate names `fs/zfs` as perf-critical, but no benchmark can see it

**Status:** OPEN (2026-08-20)

**What happens.** `boot-test.sh` compares the changed file list against its
perf-critical path rule and, on a match, prints "Performance-critical code
changed since the last benchmarked commit … CLAUDE.md requires benchmarking
these" and names the files. On 2026-08-20 it named all three of
`kernel/src/fs/zfs/{dmu,mod,tests}.rs` after the gang-block change. The
requested `./scripts/boot-test.sh --bench` run then came back **RUN CLEAN**, 86
benchmarks, nothing outside its own range.

**Why that is worth an entry.** The suite has **no ZFS benchmark**. Its 86
entries include sixteen `vfs_*`/`crypto_*` ones, and none of them reaches the
ZFS reader — it is not mounted at boot and is exercised only by its in-memory
self-test. So the clean verdict is true and also uninformative: the instrument
that was asked to check the change is structurally incapable of observing it.
This is the same failure shape as the 2026-08-19 "zero KASAN reports"
correction, where the check used a matcher the kernel never emits and therefore
could not have failed. A gate that always passes teaches the next session to
stop reading it.

**Two defensible fixes, and they are not equivalent.**

1. *Give the gate something to measure.* Add a ZFS read benchmark — block read
   with checksum verify (fletcher4 and sha256), indirect-block walk, and gang
   reassembly are all real per-block costs, and the last is new. This makes the
   existing rule honest and is the better answer if ZFS is ever mounted for
   real.
2. *Make the gate admit what it does not cover.* Have the perf-critical rule
   report which changed paths have **no** covering benchmark, so the message
   reads "no benchmark covers this" instead of silently implying one does.

(2) is the more general fix — the same blind spot presumably exists for other
paths in the rule, and nobody has checked which. (1) is worth doing regardless.
Neither is urgent: nothing is wrong with the ZFS code, and the risk is one of
false assurance, not of a missed regression that a *working* instrument would
have caught.

**Where.** The rule and its message live in `scripts/boot-test.sh`; the
benchmark registry is under `bench/`.

**[A] 2026-08-20 — a *second*, different defect in the same rule, found and
fixed.** While reading the rule for fix (2) above, its revision comparison
turned out to be `git diff --name-only "$last_commit" HEAD`, i.e. a diff between
two **commits**. Every uncommitted change was therefore invisible to it — and
since the workflow here is edit, boot-test, then commit, an unbenchmarked change
to a listed path is *most likely to be uncommitted at exactly the moment the
gate runs*. The blind spot was the common case, not a corner one.

The proof arrived in two halves one commit apart, with no code change between
them: a run built from a tree containing a modified `kernel/src/mm/user.rs` (a
listed path) printed "No perf-critical changes since the last benchmarked
commit", and the byte-identical tree printed "!! Performance-critical code
changed … kernel/src/mm/user.rs" on the next run, once the file had been
committed. Same code, opposite verdicts, decided by nothing but `git add`.

Fixed by diffing the **working tree** (`git diff <commit> -- <paths>`, no second
rev) and querying untracked files separately — a new file under a benchmarked
path is code the suite has never measured, which is the strongest reason to
escalate. The message now also says how many of the named files are uncommitted,
because `bench/history.jsonl` stamps rows with a commit hash and a row recorded
from a dirty tree names only an ancestor of what was measured. Verified against
all three cases (uncommitted modification, new untracked file, committed-only)
by extracting the function and running it against deliberately dirtied trees.

This does **not** close this entry. It was a missing *revision*; the entry above
is about a missing *benchmark*, and the gate can now correctly name a path that
still nothing measures. If anything it raises the priority of fix (2), since the
rule will now fire in strictly more situations.

**`[A]` 2026-08-20 — fix (2) is now done, and the audit it required found the
defect underneath both of them.** The gate no longer implies coverage it does
not have: `scripts/boot-test.sh` carries a per-file `BENCH_COVERAGE` map, and
its report is split into files the suite measures and files headed "covered by
NO benchmark. Running --bench will not tell you whether these got slower". The
default for anything not written down is *uncovered*, so silence is never read
as coverage. Rationale and the noise-vs-false-assurance tradeoff: design-decisions
§250.

Building that map required going through all 86 recorded benchmarks and reading
which module each one actually calls, and that turned up three things worse than
coarse granularity:

- **Four benchmarks measure a copy of the code rather than the code.**
  `net_checksum`, `tcp_checksum_v4`, `tcp_checksum_v6` and `dns_build_query` all
  time private reimplementations living in `bench.rs` (lines 6395, 6451, 6520,
  6664); one says so in its own doc comment. They cover no kernel file at all.
  Make the real label encoder in `net/dns.rs` ten times slower and every one of
  them stays green — and the old `BENCH_CRITICAL_PATHS` annotation cited them by
  name as the benchmarks `kernel/src/net` "actually guards".
- **`kernel/src/idt.rs` is measured by neither benchmark cited for it.**
  `isr_latency`'s window opens at `apic.rs:1059`, inside `handle_timer_irq` and
  past the IDT stub; `page_fault` hand-rolls map/unmap through `page_table` with
  the CPU exception explicitly excluded (`bench.rs:5450`). The same two facts
  mean **`mm/fault.rs`, the actual `#PF` handler, is unbenchmarked** despite
  "page fault handling" being a row in CLAUDE.md's perf-critical table.
- **`kernel/src/apic.rs` was not watched at all**, despite holding the only code
  `isr_latency` times. Added to `BENCH_CRITICAL_PATHS`.

The per-path benchmark annotations on `BENCH_CRITICAL_PATHS` were deleted rather
than corrected: they were a second copy of a mapping that now lives in
`BENCH_COVERAGE`, and a second copy is a second thing to be wrong. A
`report_bench_coverage_rot()` check runs on every green boot and fails the map
if it cites a benchmark the newest `bench/history.jsonl` row does not contain —
which catches renames, deletions, and the several benchmarks that print `SKIP`
and record nothing, the last of which a source grep cannot see.

**Confirmed first-hand the same day.** The `--bench` run this gate demanded for
`kernel/src/mm/user.rs` completed — PASSED, 86 benchmarks recorded, boot streak
33 — and observed nothing about the change, because no benchmark calls
`copy_to_user` or `copy_from_user`. That run was flagged `RUN CONTAMINATED` by
its own instruments (wall time 212 s against a median of 144 s; lane-B held the
boot lock for the preceding 300 s), so its one `REGRESSED, UNREPLICATED` line —
`net_veth_roundtrip` 875 ns → 1334 ns, inside its own 635–1326 ns range at the
top end — is not attributable to any code change and is logged here rather than
chased. Under the new report, that same `mm/user.rs` change would have been told
"nothing here would be measured by --bench", which is both true and 21 minutes
cheaper.

**Still open:** fix (1), a ZFS read benchmark. Also now visible as consequences
of the audit: `mm/fault.rs` and `net/dns.rs` have no honest benchmark, and four
existing benchmarks should either be pointed at the real implementations or
renamed to admit they are microbenchmarks of `bench.rs`. Pointing them at the
real code is the correct fix — the reason given for duplicating
(`tcp_checksum_bench`: "to avoid depending on tcp module internals") is a reason
to widen the module's public surface, not a reason to measure something else.

**`[A]` 2026-08-20 (follow-up) — the map's own first draft over-claimed six
times.** Auditing `BENCH_COVERAGE` entry by entry against `bench.rs` before
trusting it removed `mm/frame_owner.rs` (A/B-tested only via
`timed()`/`ab_interleaved()`, recorded nowhere — an unrecorded measurement
cannot detect a regression), `syscall/number.rs` (a compile-time constant),
`net/interface.rs` (named by four benchmarks, but only to build an `Ipv4Addr`
outside the timed closure) and `sched/task.rs`; re-pointed `fs/path.rs` from
`vfs_stat_breakdown_ns` (an atomic load on the namespace fast path) to
`_prologue`/`_resolve`, which really do run `validate_path`/`normalize_path`
over its types; and added the missing `context_switch` to
`sched/priority_rr.rs`, where `PerCpuScheduler::pick_next_local` lives. Corrected
totals: mm 4 of 47, sched 4 of 18, syscall 2 of 9, net 9 of 49.

Five of the six were over-claims, committed while writing the fix for
over-claiming. The lesson is in design-decisions §250: "this benchmark mentions
this file" is an easy question, "this benchmark would notice this file getting
slower" is the real one, and the first is habitually mistaken for the second.
Treat `report_bench_coverage_rot()` as load-bearing, and when adding a rule,
check that the call is inside the `run(...)` closure and that the benchmark is
`score`d or `track`ed rather than `run_diagnostic`'d.

Validated on a real boot: PASSED, streak 34, with the rot check running silently
on the live `finish_pass` path under `set -euo pipefail`.

**`[A]` 2026-08-21 — fix (3) done: the four copy-measuring benchmarks now measure
the kernel.** design-decisions §251. `net_checksum`, `tcp_checksum_v4`,
`tcp_checksum_v6` and `dns_build_query` each timed a hand-written copy of the
function they named, living in `bench.rs`. All four copies are deleted; the
benchmarks now call `net::ipv4::ip_checksum`, `net::tcp::tcp_checksum`,
`net::tcp::tcp_checksum_v6` and `net::dns::build_query`, the last three widened
to `pub(crate)` for exactly that reason and documented as such at their
definitions. `BENCH_COVERAGE` in `boot-test.sh` gains the four rules and
`kernel/src/net/dns.rs` as a covered file (net: 9 → 10 of 49).

Only `net_checksum`'s copy was faithful (character-equivalent to
`ipv4::ip_checksum`), so **only that series is continuous across §251**. The
other three should step:

| Benchmark | How the copy differed |
|---|---|
| `tcp_checksum_v4` | Hand-unrolled the pseudo-header into six adds; the real one builds a 12-byte array and walks it with `chunks(2)` + `.get(1).copied().unwrap_or(0)` |
| `tcp_checksum_v6` | Summed src and dst in one interleaved 8-iteration loop; the real one walks each address separately |
| `dns_build_query` | Lacked `encode_name`'s `.filter(\|l\| !l.is_empty())` — a *behavioural* difference, not just a cost one: without it a trailing-dot FQDN encodes as an invalid name |

**Expect the next `--bench` run to flag one or more of these three as REGRESSED,
and do not tune it away.** The step is real cost that the copies were hiding.
Annotate the run, then treat the post-§251 level as the new baseline. The
v4-vs-v6 comparison in particular was meaningless before: both of its sides were
bench-local copies, so the "overhead of the larger pseudo-header" it advertised
was the difference between two functions that never ran.

**`[A]` 2026-08-21 — the inverse error: coverage that exists while the gate never
asks for it.** Found by running the map *backwards* — asking which recorded
benchmarks no file rule cites. `lock_uncontended` and `lock_tracked_nested` time
`crate::sync`'s Mutex with lockdep active, so `kernel/src/sync.rs` and
`kernel/src/lockdep.rs` are genuinely covered — but neither was in
`BENCH_CRITICAL_PATHS`. A change to the kernel's lock, which sits on essentially
every hot path, therefore produced "no perf-critical changes since the last
benchmarked commit" and skipped a suite that would have caught it. This is the
mirror image of §250 (which was about the gate *asking* for files nothing
measures) and is arguably worse, because its failure is silent in the direction
that matters. Both files added to `BENCH_CRITICAL_PATHS` and to
`BENCH_COVERAGE`. `bench.rs` itself records that an `O(edges)` scan in
`record_edge` "went unread" for two runs, so this is not hypothetical.

The backwards check is worth re-running whenever benchmarks are added. After
this fix it leaves two: `rdtsc_overhead`, correctly cited by nothing because it
measures the harness rather than the kernel, and `hpet_read` — **open question**:
`kernel/src/hpet.rs` is unwatched and unmapped. It has a scored benchmark, so
adding it is cheap; whether HPET reads are hot enough to warrant gate attention
depends on how often the timekeeping path reads the counter versus the TSC,
which was not checked. Deliberately left undecided rather than guessed, since
guessing is what produced the over-claims in the first place.

Still open from this entry: fix (1), a ZFS read benchmark, and honest benchmarks
for `mm/fault.rs` (the real `#PF` handler — `page_fault` hand-rolls map/unmap
through `page_table` with the CPU exception explicitly excluded) and
`kernel/src/idt.rs` (no benchmark's measurement window contains it;
`isr_latency`'s opens inside `apic::handle_timer_irq`, well past the stub).

**`[A]` 2026-08-21 — the §251 fix introduced a *new* way to measure nothing:
`black_box` on a result does not prevent hoisting.** Postscript in
design-decisions §251. The first post-fix run (`fe9882a55`) reported
`tcp_checksum_v6` at **18 ns**, down 99% from ~1604 ns — and the run was
otherwise green, so nothing flagged it. It is arithmetically impossible: 1460
bytes is 730 checksum iterations, and 18 ns is ~70 cycles, i.e. **0.1 cycles per
iteration**, against a suite that runs at ~8 cycles/iteration under QEMU TCG on a
host whose `rdtsc_overhead` alone is 138 cycles. (SIMD cannot explain it either —
TCG emulates vector instructions, so auto-vectorisation is *slower* there.)

Cause, and it was self-inflicted: the deleted copies took their arguments from
mutable locals built in `bench.rs`, whereas calling the real function passes a
**loop-invariant immutable local to a pure function** — precisely the shape LLVM
hoists out of `run()`'s loop. The existing `black_box` was around the *return
value*, which prevents dead-code elimination but **not** loop-invariant code
motion. The guard that works is an opaque **input**:

```rust
let seg = core::hint::black_box(&segment[..]);
let _ = core::hint::black_box(crate::net::tcp::tcp_checksum_v6(seg, &src, &dst));
```

Applied to all three checksum benchmarks. Two notes on the reasoning, so neither
looks like an oversight later:

- **v4 and v6 must be guarded identically, not judged individually.** They exist
  to be subtracted from each other; one hoisted and one not would make "the cost
  of the larger IPv6 pseudo-header" the difference between a real call and no
  call — the same falsehood §251 removed, reached from the other side.
- **`net_checksum` was guarded even though it was demonstrably fine** (21 ns = 80
  cycles over 10 iterations, right on the 8-cycles/iteration line). "Not hoisted"
  is a property of an LLVM version, not of a benchmark. Its unguarded run had
  already banked the one thing it was worth — evidence that re-pointing at the
  real function did not by itself move the number — so expect a small one-time
  step there too. `dns_build_query` needs no guard: it allocates, and the global
  allocator is an opaque call that cannot be moved.

  **CORRECTION, 2026-08-21, from the verification run `d4b03ce54`: that bullet is
  wrong, and wrong in the same way as the bug it was describing.** With the input
  made opaque, `net_checksum` went 21 ns → **54 ns** (80 → 204 cycles), a 2.5×
  step, and the comparator flagged it REGRESSED. A 20-byte header is 10
  `chunks(2)` iterations, so the honest cost is ~20 cycles/iteration; the old 80
  cycles was ~8. The reason is plain in the benchmark: `header` is a **fixed
  literal array**, so `ip_checksum(&header)` is a compile-time-constant
  expression and LLVM was free to fold the entire checksum away, leaving
  `black_box` holding a constant. So this benchmark was *not* fine — it was the
  third hoisted one, not a healthy control.

  What makes this worth writing down rather than quietly amending: **"80 cycles =
  10 iterations × 8 cycles/iteration, matching the suite" was the argument, and
  it was a coincidence.** It is the identical species of reasoning that made the
  v6 result look acceptable until the cycles-per-iteration figure was computed —
  a plausible-looking number reverse-justified into evidence. Arithmetic
  plausibility is a test that can only ever *refute*; passing it is not proof
  that a window measures anything. The right answer was reached (guard it) from
  an argument that did not support it, which is luck, not method.

**What the harness did and did not do.** It reported this correctly and
prominently — `bench-history.py` printed it under `IMPROVED (>25% faster than the
suite AND outside its own recent range)` with the full comparison,
`tcp_checksum_v6: 1604ns -> 18ns (-99% vs suite, -99% raw); its own range is
1595-1610ns (median 1602ns over 8 runs)`. Nothing was hidden. The gap is *not*
detection, it is **interpretation**: a −99% move is filed under good news, and
the run PASSED. Nor would more statistics help, because statistically the number
is impeccable — `split 1st=70 2nd=70 (0%)`, a perfectly replicating level shift.
What marks it false is physics, not variance.

**The check that would catch it, and that the suite already has the ingredient
for.** `self_test_nop` — `run_diagnostic("self_test_nop", 1000, || black_box(42))`
— is an empty-closure control measuring the harness's own per-sample floor. It
came in at **72 cycles**. `net_tcp_checksum_v6_1460b` came in at **70**. A
benchmark that costs less than an empty closure is not running, and that is true
by construction rather than by threshold-picking, so it has no false positives to
tune. Note the *wrong* version of this idea, which was tried first and discarded:
"below `rdtsc_overhead` (138 cycles)" would fire on `self_test_nop` itself,
`net_ip_checksum_20b` (80), `sd_current_task_id` (106), `preempt_pair` (108) and
four others, all legitimate — the harness amortises, so the instrument's own
overhead is not the floor. The empty closure is.

Not yet implemented. It belongs in the kernel rather than in
`bench-history.py`, because `self_test_nop` is `run_diagnostic`'d and so never
reaches `history.jsonl`; the value is in memory at `finish_pass` time. One
caveat to handle when writing it: the nop control runs 1000 iterations while
scored benchmarks run 1000–5000, and more rounds means a lower minimum, so the
floor is mildly optimistic and the check should warn rather than fail.

**IMPLEMENTED, 2026-08-21** — `HARNESS_FLOOR_CYCLES` / `sample_harness_floor` /
the `FLOOR` and `BELOW-FLOOR` lines in `print_scorecard`. The warn-not-fail
caveat above was honoured, but the design as sketched here had a defect that
only showed up in a boot log, and it is worth recording because the sketch reads
as obviously safe:

> **The floor is not a property of the build. It is a property of the host's
> load during the window that measured it.**

The boot of 02:17 measured `self_test_nop` at **428 cycles**. The 01:38 boot, on
the same host and an all-but-identical kernel, measured **70**. The difference
was three lanes compiling at once. Since `bench::self_test()` runs early in boot
and `bench::run_all()` some six minutes later, a floor sampled during a build
storm and applied to a suite that ran after it lifted would have declared most
of the scorecard "not running" — the check crying wolf on its first live fire,
which for a check whose entire value is being *believed* when it fires is worse
than not having it.

Three consequences, all now in the code:

- **Sample more than once, spread apart in time** — `self_test` and again at the
  top of `run_all`, so at least one sample sits next to the benchmarks the bound
  is applied to.
- **Combine the samples with `fetch_min`, not "latest wins".** A lower floor
  flags less. For a warn-only check erring toward false negatives, that is the
  only safe direction, and it means whichever sample caught the quieter moment
  governs.
- **`u64::MAX`, not `0`, is the unmeasured sentinel**, so that "no sample" cannot
  be confused with "a floor of zero", which would silently pass everything.

Two smaller decisions worth stating. The samples go through **one** function
rather than an empty closure written out at each site — the copy risk here is
not theoretical, since `measure_access_at` already caught LLVM fully unrolling an
empty constant-trip-count loop while leaving the real one rolled, and a floor
compiled differently from the windows it bounds is a floor measuring something
else. And `ScoreEntry` carries `min_cycles` alongside `measured_ns` because the
check operates exactly where the ns conversion runs out of resolution: at
3.66 GHz every value from 68 to 76 cycles rounds to 18 or 19 ns, so a check
reading "is this at or below 70?" cannot be run against nanoseconds.

The `FLOOR` line prints unconditionally, carrying the bound and the closest
scored entry to it, for the reason the split summary prints its worst spread
when nothing is unstable: a check that emits nothing until the day it fires
leaves a reader unable to distinguish "nothing is near the floor" from "the
check is broken" — and this one has a specific silent-breakage mode, namely no
sample having been taken.

**Aside, logged because it recurred while fixing the above:** the verification
build was launched as `run-timeout.py … cargo build -p kernel | tail -25`. It hit
its 900 s timeout and was killed — and the tool reported **exit code 0**, because
a pipeline's status is `tail`'s. That is the trap already documented above at
"Two more traps in the same family", but the mitigation written for it
(`scripts/workspace-test.py`) covers only the *test* gate; an ad-hoc `cargo
build` has no such wrapper and re-opened the same hole. It was caught only
because `run-timeout`'s `TIMEOUT after 900s -- killing process tree` line
survived inside the captured text and the file was read rather than the status
trusted. **Never pipe a `run-timeout.py` invocation** — it also defeats the
30-second heartbeats, which are the only signal distinguishing "slow" from
"hung", and here 900 s was in fact too short: three lanes were building at once
(13 `rustc`, 5 `cargo`), so the kernel crate did not finish.

**This is the third instance of the same shape in `bench.rs`, which is the
argument for doing it mechanically rather than per-site.** `measure_access_at`'s
doc comment records the other two: the optimiser eliminated the stores the
canary was measuring (leaving `nop=400` against `store=244` — the store arm
*cheaper* than the empty one), and later it fully unrolled the empty
constant-trip-count loop while leaving the `write_volatile` loop rolled, putting
~11 cycles of scaffolding asymmetry inside the delta and moving the per-access
figure 4× (16.6 → 5.2 cyc) when `N` changed. Both were fixed locally —
`write_volatile` instead of `black_box`, then `black_box` on the trip count —
and the comment already states the moral: *"first the optimiser removed the
thing being measured, then it removed the thing being measured against … a
benchmark whose validity silently depended on the optimiser declining to do
something it was entitled to do."* A third instance in an unrelated benchmark
says the moral held and the response was too narrow.

### Postscript 2: the residual was not the pseudo-header either, and the cause was seven copies of one loop

**Status:** FIXED — `net::checksum` unification.

The pseudo-header rewrite (`e1de4aaaa`) was justified by a prediction, written
down before the run precisely so it could fail:

> if the 2536-cycle gap really is the stack round-trip, v4 should land near v6
> but slightly above it… If v4 instead stays well above v6, my account of the
> cause is wrong and the difference is somewhere I haven't looked.

**It failed.** v4 went 7896 → 7208 cycles (−688, −8.7%) — a real win, and enough
to put `net_tcp_checksum_v4_1460b` under its 2000 ns target for the first time
(2115 → 1904 ns) — but it stayed **1844 cycles (34%) above** v6's 5364. The
stack round-trip accounted for roughly 27% of the gap. The rest was somewhere I
had not looked.

**Where it was: the two functions were not running the same code, despite
running identical source.** `llvm-objdump` on the exact `e1de4aaaa` kernel that
produced those numbers shows only *one* of the pair as a symbol —
`tcp_checksum_v6` survives out-of-line, and its segment loop is **unrolled 2×**:

```
ffffffff808ccf30: movzwl (%rdi,%rcx), %r9d      ; word 0
ffffffff808ccf35: movzwl 0x2(%rdi,%rcx), %r10d  ; word 1
…
ffffffff808ccf53: addq   $0x4, %rcx             ; four bytes per iteration
```

`tcp_checksum` has no symbol at all — it was inlined into its callers, and the
copy inlined into the benchmark did not get the same treatment. 1460 bytes is
730 words: 365 unrolled iterations against 730 rolled ones.

**So the benchmark pair, which exists specifically to price IPv4's
pseudo-header against IPv6's, was actually reporting an unrolling decision.**
Both readings of that pair were wrong, in opposite directions and for different
reasons: before §251 it compared two hand-written `bench.rs` copies of code that
did not exist; after §251 it compared two independently-compiled copies of code
that did.

**Root cause: the loop existed seven times.** `grep` for `while i + 1 <` across
`kernel/src/net/` returned seven verbatim copies of the one's-complement data
loop — three in `ipv4.rs`, two in `ipv6.rs`, two in `tcp.rs` — plus four copies
of the IPv4 pseudo-header prologue and four of the IPv6 one. Each copy is a
separate function body to LLVM, so each independently wins or loses the
unrolling lottery, and nothing anywhere asserts they agree.

That is worse than ordinary duplication, and the reason is specific:
**duplicated code that is *supposed* to be identical invites the reader to
attribute any measured difference to the one thing that genuinely differs.**
Here the only visible difference between the two functions was the
pseudo-header, so a 34% gap "obviously" meant the pseudo-header cost 34%. The
inference is sound; the premise — that the rest was the same code — was false,
and was false in a way no amount of reading the source could reveal.

**Fix:** one canonical `kernel/src/net/checksum.rs` (`sum_bytes` / `fold` /
`finish` / `pseudo_v4` / `pseudo_v6`), and all seven sites call it. Now the two
TCP checksums differ by exactly their pseudo-header prologues and nothing else,
which is what the benchmark pair always claimed to be measuring. It also means
optimising the checksum is one edit that lifts TCP, UDP, ICMP, ICMPv6 and the
IPv4 header check together, instead of seven edits that drift apart again.

Because every checksum in the stack now funnels through one loop, a single
wrong fold or a mis-padded odd byte would corrupt all of them at once — so
`checksum::self_test()` runs at boot (`main.rs`, step 22e⅞++++n1) rather than
only under `cargo test`, which never executes against the target binary. It
pins RFC 1071 §3's own worked example (an *external* vector, so it cannot share
a bug with a test written from this code), the high-side padding of an odd
trailing byte, split-invariance of the accumulator, the stamp-and-verify round
trip, and the 64 KiB worst case that the module's no-overflow claim rests on.

**Verified in the machine code, not inferred from the benchmark.** The open
question the unification left was whether routing every site through one
`sum_bytes` would actually make them *compile* the same, given that LLVM is free
to inline it per-site and re-diverge — which is exactly what it did do: there is
still no `sum_bytes` symbol in the post-unification kernel, and `tcp_checksum`
is still inlined into `bench::run_all` with no symbol of its own. Inlining was
deliberately not pinned with `#[inline(never)]`, on the grounds that pinning it
is a decision to make *from* a measurement rather than in anticipation of one.
The measurement, taken with `llvm-objdump -d --disassemble-symbols=` on the
`faa834d4b +uncommitted` build:

```
; both inlined copies inside bench::run_all, and the out-of-line tcp_checksum_v6
ffffffff80aa7d13: movzwl (%rdx,%r9), %r11d      ; word 0
ffffffff80aa7d18: movzwl 0x2(%rdx,%r9), %ebx    ; word 1
ffffffff80aa7d31: movzwl 0x4(%rdx,%r9), %ebx    ; word 2
ffffffff80aa7d43: movzwl 0x6(%rdx,%r9), %esi    ; word 3
ffffffff80aa7d52: addq   $0x8, %r9              ; eight bytes / four words
ffffffff80aa7d56: addq   $-0x4, %r10
ffffffff80aa7d5a: jne    …+0x1b163
ffffffff80aa7d61: …                             ; + a 1x remainder loop, addq $0x2
```

All three checksum benchmarks — v4, v6 and the 20-byte IPv4 header
(`movq $0x14` = length 20, at `+0x1b29c`) — now carry byte-identical loop
shapes, unrolled 4× with a 1× tail. The 4× / 2× / not-at-all divergence is gone.
So **`#[inline(never)]` is not needed and was not added**: LLVM still inlines at
every site, but it now compiles every site the same way, which is the property
the benchmark pair actually depends on. Pinning inlining would have bought
nothing and cost a call on every short checksum in the stack.

Worth separating the two claims, because conflating them is what made the
original bug invisible: *sharing a source function* did not by itself make the
copies share code — it made the copies present LLVM with the same inlining
input, which is what made its decisions agree. That is a weaker guarantee than
one out-of-line function, and it is checked rather than assumed; if a future
LLVM diverges again the disassembly command above is the check, and
`#[inline(never)]` is the fix.

**Postscript 3, 2026-08-21: the prediction this entry recorded was wrong, and
the baseline it was measured against was never valid.**

The prediction was that unifying the checksum would close the v4/v6 gap, since
the gap was an artifact of the two sides being compiled to different unroll
factors. The first `--bench` boot after the unification (`e98846a78`) says
otherwise:

| Boot | v4 | v6 | v4/v6 | `net_checksum` (20 B) |
|---|---|---|---|---|
| `4dd776c46` 02:18 | 1717 ns | 1604 ns | **1.07** | 18 ns |
| `6b60dd0d9` 03:19 | 1713 ns | 1604 ns | **1.07** | 19 ns |
| `fe9882a55` 04:40 | 2290 ns | 18 ns | 127 | 21 ns |
| `d4b03ce54` 05:20 | 2115 ns | 1435 ns | **1.47** | 54 ns |
| `e1de4aaaa` 05:38 | 1904 ns | 1417 ns | **1.34** | 53 ns |
| `e98846a78` 06:52 | 3101 ns | 2009 ns | **1.54** | 54 ns |

The gap *widened*, from 1.07 to 1.34–1.54. And it now points the wrong way
physically: v6 sums a 40-byte pseudo-header against v4's 12, so v6 does strictly
more work per call and should be the *slower* of the two.

Three things have to be said in the right order here, because the tempting
reading — "the unification made it worse" — is not what happened.

*The 1.07 baseline was measured on partially-hoisted code, so it was never a
number to return to.* The same commit that guarded these two also guarded
`net_ip_checksum_20b`, and that one moved 19 ns → 54 ns (80 → 202 cycles). A
2.9× jump from adding `black_box` to the *input* is proof the old figure was
computed at least partly outside the timed loop. The v4/v6 pair got the same
guard in the same commit. Comparing today's guarded 1.34 against yesterday's
unguarded 1.07 compares a measurement to a non-measurement — the same error
this whole entry exists to document, committed one level up.

*The unroll claim above still holds; it was re-verified on this build, not
carried over.* `tcp_checksum_v6` is out-of-line (`0x1f4` bytes) and
`tcp_checksum` has no symbol at all, so the two sides are still inlined
differently — but their loop bodies are now equivalent instruction for
instruction: 4× unrolled, `movzwl`+`rolw` 16-bit big-endian loads on indexed
addressing, `addq $0x8` / `addq $-0x4` / `jne`, no spills on either side, 20
instructions against 19. Whatever remains is not the unroll artifact.

*The 1.54 reading is contaminated and 1.34 is the honest one.* v4's window has
mean 90043 cycles against a min of 11510 (7.8×) and a max of 146,380,043 — and
`net_ipv4_parse`, three benchmarks earlier in the same window, carries the
matching signature (mean 89854, max 179,037,442). v6's mean is 1.17× its min.
That is a host stall landing on part of the suite, and it matters more than it
looks: **under TCG the guest TSC tracks host wall-clock, so a host deschedule
inflates every iteration in the affected window, the minimum included.**
Min-of-N does not filter it, and the split check cannot see it because both
halves sit inside the same disturbed window. This is the same root cause as the
428-cycle floor sample — host load contaminating a measurement — reaching the
scored numbers rather than the floor.

So the residual is ~1.34 with provably identical inner loops. The live
hypothesis is **code placement**, not code: v4 is inlined into `bench::run_all`
(195,815 bytes) while v6 is a call to a 500-byte function, and the suite already
has three unexplained placement-shaped moves on record (`vfs_stat_deep` +39%,
`vfs_stat_3comp` +37%, `isr_latency` −38% — mixed directions, the signature of
layout rather than work). `scripts/straddle-check.py --compare <old> <new>` is
the tool. Not yet run; this is the next thing to do on this thread, and it is
recorded here rather than presented as resolved.

Two corollaries for the harness itself:

- **Mean/min ratio is a usable contamination detector and is not currently
  checked.** v4 at 7.8× and `net_ipv4_parse` at ~394× stand out sharply against
  v6's 1.17×. A scorecard line flagging windows whose mean is some large
  multiple of their min would have marked both of these as untrustworthy
  *before* anyone reasoned about the numbers. Worth adding next to the
  below-floor check, which catches the opposite failure.
- **A benchmark pair meant to be compared should be measured adjacently and
  reported with its dispersion**, since the pair's whole value is the ratio and
  a stall on one side forges it.

**Postscript 4, 2026-08-21, one hour later: postscript 3 was itself wrong, and
the second boot proves it. The gap does not exist.**

Postscript 3 concluded "residual ~1.34 with identical inner loops, hypothesis is
code placement" from a single `--bench` boot. A second boot on the merged tree,
same kernel source, says:

| | v4 | v6 | ratio |
|---|---|---|---|
| boot 1 `e98846a78` | 11510 cyc | **7458 cyc** | 1.54 |
| boot 2 `40a379b55` | **7228 cyc** | **7458 cyc** | **0.97** |

**v6 measured 7458 cycles in both boots, to the cycle.** v4 fell from 11510 to
7228. So v4 is now marginally *faster* than v6 — which is the physically correct
ordering, because v4 sums a 12-byte pseudo-header where v6 sums 40, and 230
cycles is a fair price for 14 extra words plus a call. The original §251
prediction — that unifying the checksum would close the gap — **was correct**.
There is no residual, no placement effect, and nothing for `straddle-check.py`
to find here. Postscript 3's "1.07 → 1.34 → 1.54" was a table of how badly each
boot's v4 window happened to be disturbed.

What makes this worth keeping rather than quietly deleting is that postscript 3
identified the mechanism correctly and *still* drew the wrong conclusion from
it. It said, accurately, that the guest TSC tracks host wall-clock under TCG and
that v4's window was contaminated. Then it treated 1.34 as "the honest one"
because it was the least contaminated reading available — instead of concluding
that no reading was trustworthy and taking another boot. **Picking the best of a
set of known-bad measurements is not the same as having a good one**, and the
cost of finding out was one boot: 151 seconds.

The refined mechanism, now that there are two boots to compare:

- **Contamination moves between benchmarks from boot to boot.** Boot 1 hit v4
  (mean 7.8× min) and spared v6 (1.17×). Boot 2 hit v6 (mean 81091 against min
  7458, max 1.37e8) and spared v4. It is not a property of either benchmark.
- **Min-of-N filters a partially-disturbed window and cannot filter a wholly
  disturbed one.** Boot 2's v6 split reads `1st=9886 2nd=7458 (32% UNSTABLE)` —
  the stall covered the first half, the second half was clean, and the minimum
  correctly picked up the clean value, landing on the same 7458 as boot 1. Boot
  1's v4 split reads `1st=11510 2nd=12010 (4%)` — both halves inflated, no clean
  draw anywhere in the window, so the minimum had nothing good to find, and the
  split check saw two consistent halves and passed it.
- Therefore **the split check's silence means "no *change* mid-window", not "no
  contamination"**, and a low split percentage on a window whose mean is many
  times its min is the specific combination to distrust. Boot 1's v4 was exactly
  that: 4% split, 7.8× mean/min.

This strengthens rather than weakens postscript 3's harness proposal, and
sharpens it: **flag any window whose mean exceeds its min by a large factor, and
flag it hardest when the split check is clean**, since that pairing is invisible
to every check the suite currently runs. Both bad windows here would have been
caught (7.8× and 10.9×) against v6-in-boot-1's honest 1.17×.

Standing correction to how this file gets written: two of the three conclusions
in this §251 thread were drawn from one boot each, and both were wrong. The
below-floor check was designed against the 428-cycle observation precisely
because one sample is not a measurement — and postscript 3 then drew a
conclusion from one sample anyway. **A performance claim in this file needs two
boots, and if the two disagree the answer is a third, not an average.**

**Two general lessons, both of which cost a full measure-and-conclude cycle
here:**

1. **"Identical source" is not "identical code."** Two functions with
   byte-identical loop bodies can differ by 2× in emitted work depending on
   inlining context. Any benchmark that compares two functions and attributes
   the difference to their *visible* difference is assuming the compiler treated
   the invisible parts equally — and that assumption is checkable in about
   ninety seconds with `llvm-objdump -t` plus
   `--disassemble-symbols=`. Do that before writing the causal story, not after
   the story fails.
2. **A prediction that fails is worth more than the fix that prompted it.** The
   `e1de4aaaa` optimisation was correct and is kept. But had it not carried an
   explicit falsifiable claim about *how much* it should recover, "−8.7%, target
   met, done" would have closed the task with the actual defect untouched and
   the benchmark still lying. The habit worth keeping is not "predict the
   direction" — direction was never in doubt — it is **predict the magnitude**,
   because that is the part that can be wrong while everything looks fine.
