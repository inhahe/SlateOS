# Known Issues — Resolved Archive

Entries from `known-issues.md` that are **fixed and verified**, moved here so
that file stays a list of what is *still* wrong. Nothing is deleted: an entry
keeps its full text, its `**Status: FIXED**` stamp, and its commit hashes, so
the reasoning behind a fix is still greppable from one place.

**When to move an entry here:** it is fixed, and the fix has been on `main`
through a full boot test. Until then it stays in `known-issues.md` with a
`**Status: FIXED**` stamp — a fix that has not survived a boot is a claim, not
a resolution.

**Who may move one:** the lane that owns it, into that lane's section below.
This file is lane-partitioned like the others (`roadmap.md` rule 3,
`design-decisions.md` §437), so three lanes archiving at once land at three
different offsets and the merge is automatic.

The migration is **incremental**, not a one-shot sweep. As of 2026-08-16
`known-issues.md` held 999 `###` entries plus 77 `##` ones, of which roughly
777 read as resolved — about 55,000 of its 73,000 lines. Lane C's are below;
lanes A and B have been asked to move theirs (`requests/c-a-…`,
`requests/c-b-…`). Until they do, resolved entries still live in both files,
so **grep both**.

---

# Lane A

## Liveness watchdog reported `SYSTEM HANG` on healthy boots and disarmed itself, disabling hang detection for the rest of boot (lane A)

**Status: FIXED 2026-08-15** (lane A), reported by lane C in
`requests/c-a-liveness-system-hang-false-positive.md`. The total-hang branch of
`sched::liveness_check` now (a) refuses to report while kernel-side progress
counters are advancing and (b) no longer disarms the watchdog when it does
report.

**What was wrong, and which half actually mattered.** The report itself was a
false positive, which is bad; the *disarm* that accompanied it was the real
damage. `liveness_check`'s total-hang path did
`LIVENESS_ARMED.store(false, Ordering::Release)` before printing, and
`liveness_boot_deadline_check()` early-returns on `!LIVENESS_ARMED`. So a single
spurious report at ~140 s of armed time switched off not just the two progress
detectors but the **wall-clock boot deadline** — the backstop whose own doc
comment calls it the thing that "detects *any* hang mode (including the
ping-pong livelock the progress detectors are structurally blind to)". The
remaining ~600 s of that boot had no hang detection at all. The vanished
30/60/90/120 s breadcrumbs in the log are the proof it really went dark rather
than merely stopped reporting.

The sibling busy-livelock branch, two branches down, had already documented the
correct policy and already obeyed it: *"Keeping the watchdog armed means a false
positive here cannot disable hang detection for the remainder of boot."* The
total-hang branch was the outlier.

**Why it fired on a healthy boot.** The branch requires, for three consecutive
5 s intervals, that `USEFUL_WORK_TICKS` not advance **and** that serial output
not advance. `LIVENESS_LAST_OUTPUT`'s own doc comment already conceded the first
condition holds during normal boot — a starting ring-3 process "spends nearly
all its wall time inside the kernel on its own behalf (ELF load, demand-paging
storm, filesystem I/O)", so ticks land in kernel mode with an empty run queue.
The silence gate was added by `BUG-LIVENESS-DEADLINE-FALSE-FIRE` (2026-07-27) on
the premise that "this kernel narrates its boot continuously, so a *silent*
interval means execution really has stopped." **That premise does not survive a
large kernel-side or ring-3 operation**, which narrates nothing for tens of
seconds. Both conditions then coincide and the detector fires.

**Three independent sightings, not one.** Lane C's merge boot of 2026-08-15
(`useful_work=82`, fired just before a 3.5 MB fastpy ELF was demand-paged off
ext4); `known-issues.md` (the BUG-LIVENESS-DEADLINE-FALSE-FIRE entry's later
addendum) with `useful_work=6`, dismissed at the time as "non-fatal … and the
boot then recovered"; and — found while fixing this — lane A's own calibration
boot of 2026-08-16, `useful_work=349`, firing immediately after
`[spawn] Running link()/linkat no-follow symlink test (kernel, ext4 /mnt)` with
`preempt_disable_depth=3`. That third one is what settles the diagnosis: the
workload in the window was an ext4 test, i.e. block I/O, and the machine was
demonstrably deep inside the kernel doing it.

**The fix.** A new `LIVENESS_LAST_KWORK` counter sums page faults resolved
(`mm::fault::fault_stats`) plus block-I/O operations completed
(`blkdev::io_stats`) — both plain relaxed atomic loads, because a watchdog that
took the `SCHED` lock could deadlock against the very hang it exists to report.
If that total advanced during the interval, the total-hang branch resets its
counter and returns.

Two design points are worth keeping:

- **Only the total-hang branch consults it, and that is a definitional argument
  rather than a tuning one.** That branch asserts "no task-level forward
  progress, all CPUs idle-ticking"; a resolved page fault or a completed disk
  read *contradicts* that assertion, so the gate removes a false statement. The
  busy-livelock branch asserts something else — "a task is monopolizing a CPU" —
  which is perfectly consistent with a fault storm, so the same gate there would
  blind a correct detector instead of fixing an incorrect one.
- **Bounding the report count replaces disarming as the way to stop log spam.**
  `LIVENESS_MAX_HANG_REPORTS = 3`, with the stall counter reset after each
  report so the next one needs another full `LIVENESS_ALERT_COUNT` of continuous
  stall. Three rather than one because the second and third task-table dumps
  carry information the first cannot: whether the task states are *changing*,
  which is what separates a frozen system from a merely slow one.

**Why it survived so long: the drill existed for the other branch only.**
`test_liveness_watchdog` had a busy-livelock drill that explicitly asserted the
watchdog stays armed, and **no drill at all** for the total-hang branch — so the
branch kept a policy its sibling had already rejected in writing. A drill now
covers it, and asserts both halves: that kernel-side progress suppresses the
report, and that when the report does fire `LIVENESS_ARMED` is still true.

The second half is not redundant — **it is what stops the first half passing
vacuously.** On its own, "no report fired" is satisfied by an interval that was
merely *not silent*, since that path returns early with the same zeroed counters
and never reaches the kernel-progress gate at all. Asserting a report *does*
fire under otherwise identical conditions proves the environment is silent, so
the only remaining difference between the two halves is the one integer the
first half rewinds. Both halves also pin the progress baseline explicitly each
interval rather than trusting ambient quiet, so neither depends on whether a
page fault happens to land in the window.

**And the contract is machine-checked now.** `BUG-LIVENESS-DEADLINE-FALSE-FIRE`
had *required*, since 2026-07-27, a boot log containing no `SYSTEM HANG` /
`BOOT DEADLINE EXCEEDED` line and containing the `disarmed after …` measurement
without the FALSE POSITIVE warning. That requirement lived only in prose:
`boot-test.sh` grepped for `BOOT_OK` and nothing else, which is why a run
violating both halves still exited 0. `check_liveness_failures()` now fails the
boot on any of those lines. Because the deliberate drills would otherwise trip
it on every healthy boot — and an assertion that always fires gets deleted,
leaving the contract unchecked again — the drills print a `(self-test) ` infix
and the harness matches only the real shape.

**Bonus fix: the idle task was wearing a userspace test's name.** The hang dump
showed `tid=0 state=Running cpu=0 prio=31 … name="prctl-batch269"`, reading as
though a userspace prctl test were the task running at the moment of the "hang" —
actively misleading in the one dump you most want to trust. `prctl(PR_SET_NAME)`
routes to `current_task_id()`, which is 0 in kernel context, and the self-test's
comment asserted that "kernel context has no PCB so the name isn't actually
stored". That was simply false: task 0 is the BSP idle task and it very much
exists, so the store landed on it. `sched::set_task_name` now refuses task 0
outright — making the comment's claim true by construction rather than by luck —
and the syscall self-test asserts the refusal, then exercises the storage
round-trip on the lowest non-zero task id instead.

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

### [A] B-CONSOLE-LOCK-IS-TAKEN-FROM-A-HARD-IRQ-WITH-A-PLAIN-LOCK. The keyboard ISR echoes through `CONSOLE.lock()`, so any task interrupted while holding the console wedges the CPU forever, silently — 2026-08-14 — **FIXED** (`a18ea83a9`)

> **Resolution.** The fix landed in the *same commit* that added this entry,
> so everything below is written in the pre-fix voice ("Proper fix. Convert
> …") and describes work that is **already done**. Re-verified 2026-08-14:
> `kernel/src/console.rs` imports `crate::sync::Mutex` (line 71), all three
> statics are `Mutex::named` (`COLOR_SCHEME` 134, `SCROLLBACK` 512,
> `CONSOLE` 720), and the file now contains **45 `lock_irqsave()`
> acquisitions (34 + 7 + 4) and zero plain `.lock()` calls and zero
> `spin::` references**. The heading said `OPEN` until this correction —
> see the process note at the end of this entry.

**Symptom (predicted, not yet observed).** The machine stops dead with no
output, no panic, no stall report. Nothing is printed because the CPU is
spinning inside an interrupt handler on a lock whose holder is the very
frame the interrupt suspended.

**The chain.** All five links are `grep`-verifiable, no inference:

| # | Site | Call |
|---|---|---|
| 1 | `kernel/src/ioapic.rs:730` | `pub extern "C" fn handle_device_irq(irq: u32)` — **hard IRQ context** |
| 2 | `kernel/src/ioapic.rs:746` | → `keyboard::handle_scancode()` |
| 3 | `kernel/src/keyboard.rs:282` | → `handle_normal()` |
| 4 | `kernel/src/keyboard.rs:461` | → `push_char()` |
| 5 | `kernel/src/keyboard.rs:489,490,491,497` | → `crate::console::putchar(ch)` |

and `console::putchar` (`kernel/src/console.rs:850`) opens with
`let mut con = CONSOLE.lock();` where `CONSOLE`
(`kernel/src/console.rs:679`) is a **raw `spin::Mutex`** and `lock()` is
the plain, interrupts-enabled acquire.

So: task T calls any of the 45 `CONSOLE.lock()` sites. While T holds the
lock, IRQ 1 fires **on the same CPU**. The ISR runs on T's stack, reaches
`putchar`, and spins on a lock that only T can release — and T cannot run
again until the ISR returns. Permanent, silent, single-CPU deadlock.
`SCROLLBACK` (line 478) and `COLOR_SCHEME` (line 107) are raw the same
way; they are reachable from the same ISR because `putchar` → `scroll_up_locked`
→ `SCROLLBACK.lock()` (line 2271).

**Why it is silent.** This is the distinguishing feature, and the reason
it is worth writing down rather than just fixing. Both instrumented lock
types route contention through a 30-second stall detector that fires from
*inside* the spin loop (`kernel/src/sync.rs`, `STALL_SECONDS` at line 73).
A raw `spin::Mutex` has no such detector: it spins forever and reports
nothing. A wedge on this lock therefore produces exactly zero evidence.

**Why it has not been hit constantly.** Exposure is reduced — but not
eliminated — by two accidents:

* kshell drives the keyboard with `ECHO_ENABLED` off, so the shell's own
  key handling does not take the console lock from the ISR. The default
  canonical-TTY echo-on path does.
* The window is only as wide as a console critical section, and most of
  the 45 are a few instructions. `write_str` over a long line, and any
  call that scrolls (full-screen `memmove` plus a scrollback `Vec::push`
  that can realloc), are the wide ones.

Reduced exposure is not a defence. This is a "works until it doesn't"
bug: the odds scale with typing during output, which is precisely what an
interactive shell does.

**Contrast: `serial.rs` already defends this exact case, three ways.**
`serial::_print` (`kernel/src/serial.rs:204`) wraps the whole acquire in
`crate::cpu::without_interrupts(...)`, claims a per-CPU `IN_PRINT` flag
*before* taking the lock, and falls back to a lock-free
`SerialPort::emergency()` on re-entry. Its doc comment (lines 190–198)
describes the failure mode verbatim: "a garbled report can be read, a
deadlock cannot." `console.rs` has none of the three. The two files
diverged; only one of them was thought about.

**Proper fix.** Convert `CONSOLE`, `SCROLLBACK` and `COLOR_SCHEME` from
`spin::Mutex` to `crate::sync::Mutex` and change every acquisition to
`lock_irqsave()`. That is the structural fix, not a mitigation: masking
interrupts on the local CPU for the hold makes the reentrant arrival
*impossible* rather than merely unlikely, and it is categorical — it
protects against any future IRQ-context console user, not just the
keyboard. It also lands the locks in the instrumented type, so a future
wedge here reports itself instead of hanging mutely, and lockdep gets the
`CONSOLE → SCROLLBACK` edge.

This matches Q24's taxonomy (design-decisions.md §70): `CONSOLE` is a
**non-leaf** lock — `scroll_up_locked` (line ~2255) takes
`SCROLLBACK.lock()` at line 2271 while the caller holds `CONSOLE` — so it
takes `crate::sync::Mutex`, not `PreemptSpinMutex`.

**Invariant the fix relies on, recorded so it is not silently broken
later.** `lock_irqsave` closes the *interrupt* window; it does not close
an *exception* window, because `cli` does not mask faults. The fix is
therefore complete only while no exception handler prints to the console.
That holds today and was checked, not assumed: the only `console::` callers
outside `console.rs` are `keyboard.rs`, `kshell.rs` and `ipc/io_ring.rs`,
and the `#[panic_handler]` (`kernel/src/main.rs:5952`) executes `cli()` and
then uses `serial_println!` exclusively. If a fault handler is ever taught
to write to the console, it must first grow a per-CPU re-entrancy guard in
the shape of `serial.rs`'s `IN_PRINT`.

**Cost of the fix, stated honestly rather than glossed.** `lock_irqsave`
masks interrupts for the whole hold, so the widest console critical section
is now also the widest interrupts-off window on the system: `scroll_up_locked`
(`kernel/src/console.rs:~2296`) does a ~3 MiB framebuffer `ptr::copy` plus a
per-pixel clear of the bottom row while holding `CONSOLE`. That is hundreds
of microseconds to low milliseconds. This is accepted, for three reasons:

* It is task context, not ISR context, so it is not measured against the
  10 µs ISR budget.
* It cannot be narrowed while the lock is held — dropping `IF` mid-scroll
  to shorten the window is precisely the reentrant arrival the fix exists
  to prevent, so any "optimisation" here reintroduces the deadlock.
* The alternative (leave the lock raw and hope the ISR never lands inside
  a critical section) trades a bounded latency cost for an unbounded,
  silent, permanent hang. That is not a trade.

The console's other whole-screen loops were already written to drop the
lock first (`clear_screen` at ~868, `apply_scheme` at ~312 both capture the
geometry, `drop(con)`, then blit) — so the exposure really is limited to
the scroll path.

Usefully, the conversion also makes the cost *measurable*: `lock_irqsave`
feeds `crate::cpu::irqoff_tracker`, so `irqoff` in kshell now reports the
max interrupts-off duration this path actually produces, instead of it
being invisible. That number is the evidence that should drive
TD-CONSOLE-ECHO-RUNS-IN-HARD-IRQ-CONTEXT below.

**Not related to B-FORKEXEC-BOOT-HANG.** Tempting, but no: the
diagnostics that went missing in that hang were `serial_println!`, and
serial is a wholly separate lock with its own (working) defence. Recording
the non-link so a later session does not "solve" that hang by pointing at
this fix.

**Separate concern, deliberately not folded into this fix.** Rendering
glyphs into the framebuffer from inside a hard IRQ handler blows CLAUDE.md's
"total ISR latency < 10 µs" target by orders of magnitude, deadlock or no
deadlock. The right shape is Linux's: the ISR queues the character and a
bottom half does the echo, after which the console lock is task-context-only
again. That is a different change with a different risk profile, so it is
logged below as TD-CONSOLE-ECHO-RUNS-IN-HARD-IRQ-CONTEXT rather than
smuggled into a deadlock fix.

**Process note — why this entry said `OPEN` for a bug that was already
fixed.** Both this entry and the RTL8139 one below were written *while
investigating*, in the future tense ("**Proper fix.** Convert `CONSOLE` …"),
and then committed **together with the fix that carried them out**. The prose
was accurate when drafted and stale by the time it was committed, and the
`— OPEN` in the heading — the only part anyone skimming the file actually
reads — was never flipped. Both entries then sat mislabelled until a later
session went looking for "open Lane A bugs to fix", picked these two off the
`grep '— OPEN$'` list, and found the work already done.

That is an expensive failure mode in both directions: it invites duplicate
work, and worse, it corrodes trust in the file — if the `OPEN` list contains
fixed bugs, the natural correction is to stop believing the file, which is the
opposite of what a bug tracker is for. Note it is the same shape as the
benchmark failures recorded later in this file
(`B-BENCH-CANARY-CERTIFIES-CLEAN-RUNS…` and its three predecessors): **a
status that is never re-checked degrades into a status that is merely
asserted.** The heading is a claim about the world; committing it unchanged
alongside a fix makes it a claim about nothing.

The rule that prevents it: **when one commit both writes up a bug and fixes
it, the heading must be written in its post-fix state in that same commit.**
If the fix is not complete there, the write-up should say what remains rather
than inheriting a blanket `OPEN`. The cheap standing check is
`grep -n '— OPEN$' known-issues.md`, confirming each hit against the code
before trusting it — which is how these were caught.

**How widespread it was, measured rather than assumed.** Having found two, the
obvious question was whether the rest of the `OPEN` list could be trusted, so
all six Lane-A `— OPEN` headings were checked against the code. **Three of the
six were stale** — this one, the RTL8139 entry below, and
`TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE`, the last of which had
even accumulated an internal `**Closed 2026-08-14 …**` paragraph while its
heading still said `OPEN`. So the file's headline status was wrong for **half**
the open list. That is the number worth remembering: this was not two
oversights, it was the normal outcome of the workflow that produced them, and
the only reason it looked like an exception is that nobody had counted.

### [A] B-RTL8139-SEND-SPINS-FOR-THE-EVENT-WHOSE-HANDLER-WANTS-THE-LOCK-IT-HOLDS. `send()` polls 100 000 times for TX-complete while holding the lock `handle_irq` blocks on — 2026-08-14 — **FIXED** (`64f7d2fd9`)

> **Resolution.** As with the console entry above, the fix landed in the same
> commit that added the write-up, so the "**Proper fix.**" paragraph below
> describes work already done. Re-verified 2026-08-14: all four `DEVICE`
> acquisitions in `kernel/src/rtl8139.rs` are `lock_irqsave()` (lines 364,
> 375, 573, 605) and none is a plain `lock()`. The *deadlock* is closed; the
> 100 000-iteration busy-wait inside `send` is deliberately **still there**
> and is still worth fixing — see the final paragraph of this entry for the
> block-and-wake rewrite, which remains open work.

**Found by auditing the bug *class* rather than the bug.** After fixing
B-CONSOLE-LOCK-IS-TAKEN-FROM-A-HARD-IRQ above, the obvious question was
whether the console was the only place a hard-IRQ handler blocks on a
lock that task context also takes. `handle_device_irq`
(`kernel/src/ioapic.rs:731`) is the sole hard-IRQ device dispatch, so the
audit is bounded and can be made *complete* rather than sampled:

| Callee in `handle_device_irq` | Verdict |
|---|---|
| `cputime::enter_irq` / `exit_irq` | lock-free (atomics) |
| `ktrace::record` | lock-free |
| `keyboard::handle_scancode` (irq 1) | was the console bug — now fixed |
| `mouse::handle_irq` (irq 12) | lock-free |
| `virtio::blk::handle_irq` | lock-free (atomics + port I/O) |
| `virtio::net::handle_irq` | lock-free (its `DEVICE` is never touched from IRQ) |
| `rtl8139::handle_irq` | **plain `DEVICE.lock()` — this entry** |
| `irq_notify`, `irq_storm::record_irq` | lock-free |
| `sched::try_wake` | `SCHED.try_lock()` — correct by design, returns false and raises a softirq |
| `apic::eoi` | lock-free |
| `softirq::process_pending` | re-enables interrupts first — different class, see below |

One finding. `e1000` was checked too and is clean *for a different reason*:
it has no `handle_irq` at all (it is polled), so its `DEVICE` is
task-context-only.

**The bug.** `rtl8139::handle_irq` (`kernel/src/rtl8139.rs:553`) does
`let guard = DEVICE.lock();` in hard-IRQ context. The same `DEVICE`
(line 181) is taken in task context by `with_device` (line 361), which is
what `send` (366) and `recv` (372) go through — and `with_device` holds
the lock across the whole closure.

**Why this one is worse than the console.** Look at what `send` does while
holding the lock (`kernel/src/rtl8139.rs:392`):

```rust
// Wait for the descriptor to become available (OWN bit clear
// means hardware finished with it).
for _ in 0..100_000u32 {
    let status = unsafe { port::inl(self.io_base + status_reg) };
    if status & TX_STATUS_OWN == 0 { break; }
}
```

The OWN bit is cleared by the hardware finishing the previous transmit —
**which is precisely the event that raises the TX-complete interrupt.** So
the code spins, holding `DEVICE`, waiting for the exact hardware event
whose interrupt handler will block on `DEVICE`. This is not a narrow race
window that a busy system might hit; it is a loop that waits for the
trigger while holding the trigger handler's lock. On any TX-active link
the interrupt lands inside that loop essentially by construction.

**Why it hasn't been seen.** The RTL8139 is not the NIC the QEMU boot test
runs — virtio-net and e1000 are — so `handle_irq` never fires here. The
driver is untested-in-anger, not correct.

**One mitigating difference from the console bug:** `DEVICE` is already a
`crate::sync::Mutex` (`use crate::sync::Mutex` at line 26), so the 30-second
stall detector *will* fire and name the lock. This hangs loudly rather than
silently. It still hangs.

**Proper fix.** Change all `DEVICE` acquisitions in `rtl8139.rs` to
`lock_irqsave()` — the same structural fix as the console, for the same
reason. Note that on this driver `lock_irqsave` inside `send` is not merely
protective: it is what makes the poll loop terminate, because with the
interrupt masked the handler cannot run at all until `send` releases, and
the OWN bit is observable by polling regardless of whether the interrupt
was delivered.

Separately, the 100 000-iteration poll while holding a lock is bad shape on
its own merits (it is a busy-wait for a device with no bound in time). The
right long-term structure is the one `virtio::blk` already uses: the ISR
acknowledges at the device with atomics only and wakes a task, and the
descriptor wait becomes a block-and-wake rather than a spin. That is a
driver rewrite, so it is not folded into the deadlock fix.

**The exception class was audited too, and is clean — by design, not by
luck.** This matters because `cli` does not mask faults, so an exception
handler that blocks on a task-held lock cannot be fixed by `lock_irqsave`
at all; it has to use `try_lock`. Checked:

* `idt.rs` itself takes **no** locks (0 acquisition sites in the file).
* `mm::fault::resolve` (`kernel/src/mm/fault.rs:262`) uses
  `KERNEL_AS.try_lock().ok_or(KernelError::PageFault)?`, with a comment
  naming the exact hazard: *"if we faulted while holding this lock (e.g.,
  during VMA manipulation), the fault is in critical code and cannot be
  resolved."* `add_kernel_vma`/`remove_kernel_vma` (290, 299) keep the plain
  `lock()`, correctly — they are task-context-only and the fault path never
  blocks on them.
* `proc::pcb::try_resolve_fault` (`kernel/src/proc/pcb.rs:5370`) uses
  `PROCESS_TABLE.try_lock()`, and further drops the guard *before* CoW
  resolution because that path allocates.

**The pattern worth noticing.** The memory-management code was written with
this hazard in mind throughout — `try_lock` plus a comment explaining the
re-entrancy every time. The device and console code was not: plain `lock()`
everywhere, no comment, no defence. The discipline exists in one half of the
tree and is absent from the other, which is why both bugs found so far are
in drivers/console and none are in `mm`. When auditing further, weight
driver code accordingly.

**Audit scope note, so the next session knows what was *not* covered.**
Softirq handlers (`softirq::process_pending`, called after EOI with
interrupts *re-enabled*) are a third class: they can be interrupted by a
further device IRQ, so a lock shared between a softirq handler and a
hard-IRQ handler has the same failure mode. That intersection is currently
empty because the only hard-IRQ lock acquisition in the whole tree is the
`rtl8139` one above — but it stops being empty the moment another ISR
learns to take a lock, so the check has to be redone whenever one does.

### [A] TD-CONSOLE-ECHO-RUNS-IN-HARD-IRQ-CONTEXT. Keyboard echo renders glyphs to the framebuffer from inside the IRQ 1 handler — 2026-08-14 — ✅ FIXED 2026-08-14 (`kernel/src/keyboard.rs`)

**Fix.** The ISR no longer renders. `push_char` now hands the byte to
`queue_echo`, which filters the non-echoing keys, pushes into a new 256-byte
SPSC echo ring, and submits a single `drain_echo` work item to the kernel
workqueue. `drain_echo` runs in the worker *task* context and does the
`console::putchar` calls there.

**Why a workqueue and not a softirq** — the mechanism that looks like the
obvious choice is the wrong one, so this is worth stating. A softirq runs on
the interrupted task's kernel stack with that task suspended mid-execution, so
it must never block on a lock the interrupted task might hold;
`softirq.rs`'s own contract requires handlers to use `try_lock`. Echo needs
the console lock unconditionally, so routing it through a softirq would have
converted the hard-IRQ deadlock into a softirq deadlock and looked like
progress. The workqueue is explicitly "deferred work in process context …
may sleep, take mutexes, allocate", which is what rendering needs. Linux
reaches the same conclusion: `tty_flip_buffer_push` defers to `flush_to_ldisc`
on a workqueue, not to a softirq.

**Details worth keeping.**

* *Submission is coalesced.* One work item per keystroke would exhaust the
  workqueue's 64-item capacity during a key-repeat storm and start dropping
  **other subsystems'** work — a much worse failure than slow echo. A
  `ECHO_DRAIN_SCHEDULED` flag means a burst submits once.
* *The flag is cleared before draining, not after.* A producer that pushes
  after the clear sees it clear and submits a fresh drain; the worst case is
  one redundant drain finding an empty ring. Clearing afterwards would leave
  a byte stranded until the next keystroke.
* *A failed submit un-latches the flag.* Otherwise a single full-queue moment
  would latch `SCHEDULED` true with nothing scheduled to clear it, and echo
  would be dead for the rest of the boot.
* *Early boot falls back to inline rendering.* `keyboard::init` runs ~700
  lines ahead of `workqueue::init`, so there is a real window with no worker
  to defer to. Inline rendering there is harmless — no userspace, no latency
  budget in force, no contention for the console lock — and `is_running()` is
  monotonic, so the two paths cannot interleave and reorder output.
* *Dropped bytes are counted, not ignored.* A silent drop shows up to the user
  as randomly missing characters while the input ring still holds the byte —
  i.e. the shell acts on input that was never displayed. `ECHO_DROPPED` makes
  that visible.

**Tested** by a new `echo_ring_self_test` (run from `keyboard::self_test`)
covering FIFO order, exact capacity (`SIZE-1`, one slot sacrificed to
distinguish full from empty), refusal past capacity, and drop accounting. It
masks interrupts for the duration because the producer under test *is* the
IRQ 1 handler — a keystroke arriving mid-test would push into the same ring
and make the assertions describe something other than what they name.

**Consequence for the earlier deadlock fix.** The console lock is now
task-context-only on this path, so the `lock_irqsave` from
B-CONSOLE-LOCK-IS-TAKEN-FROM-A-HARD-IRQ-WITH-A-PLAIN-LOCK is belt-and-braces
rather than load-bearing. It stays: it is what makes the guarantee categorical
for any future IRQ-context printer, and re-introducing the bug should require
someone to actively remove a safeguard rather than merely forget one.

---

*Original entry:*

**The debt.** `handle_device_irq` → `keyboard::handle_scancode` →
`push_char` → `console::putchar` (chain tabulated in
B-CONSOLE-LOCK-IS-TAKEN-FROM-A-HARD-IRQ-WITH-A-PLAIN-LOCK above) does the
full console pipeline — escape-sequence state machine, glyph blit, and on
the last column a whole-screen scroll (`memmove` of the framebuffer plus a
scrollback `Vec::push` that can hit the heap allocator) — with the CPU
inside a hard interrupt handler.

**Why it matters.** CLAUDE.md's interrupt-dispatch budget is "total ISR
latency < 10 µs, deferred work via softirq/tasklet equivalent". A
full-screen scroll at 1024×768×32bpp is ~3 MiB of `memmove`; it is not
within three orders of magnitude of 10 µs. Every keystroke that lands on
the bottom line therefore stalls the timer tick and every other device.

**Proper fix.** Split the echo out of the ISR the way Linux splits n_tty:
the handler decodes the scancode and pushes the resulting byte(s) into the
existing input ring, and a bottom half (the tty/console task) drains the
ring and does the rendering in task context. Once the console lock is
task-context-only, the `lock_irqsave` from the deadlock fix above becomes
belt-and-braces rather than load-bearing — keep it anyway, since it is what
makes the guarantee categorical for any *future* IRQ-context printer.

**Why not done now.** It changes echo latency and ordering
(character-visible-at-keystroke becomes character-visible-at-next-drain),
which is a user-visible interactivity change and wants its own boot test
and its own commit. The deadlock is the urgent half and is fixed
independently.

### B-MOUNT-ACCEPTS-UNREACHABLE-MOUNT-POINTS. `Vfs::mount` succeeds when the mount point's parent does not exist, producing a filesystem nothing can reach — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/fs/vfs.rs`, `kernel/src/fs/overlay.rs`)

**Symptom.** The overlay filesystem self-test failed on every boot, so the boot
test reported `Boot test FAILED (BOOT_OK reached but a self-test failed)`:

```
[overlay]   commit: OK (applied 1 changes)
[vfs] Mounted overlay filesystem at '/mnt/ovl-cow-test' (rw)
WARNING: Overlay filesystem self-test failed: NotFound
```

Every one of the twelve preceding overlay sub-tests passed, including
`read lower: OK` reading the very file that then failed. The failure was the
first VFS-routed read after the mount.

**Why it looked like an overlay bug and was not.** The log line says the mount
succeeded, so the investigation naturally went to the overlay engine and its
VFS adapter — `normalize_rel`, `layer_join`, `OverlayFs::stat`/`metadata`, the
page-cache route in `read_file_routed`. All correct. The mount really had
succeeded; it was simply unreachable.

`Vfs::resolve_inner` walks every non-final path component and requires each to
exist in its containing filesystem, where "exists" includes being a mount point
(`resolve_mount`'s longest-prefix match maps it to the mounted fs). Resolving
`/mnt/ovl-cow-test/file_a.txt` therefore fails on the **first** component:
nothing creates `/mnt` at boot, `/mnt` is not itself a mount point, so
`lstat("mnt")` against the root memfs returns `NotFound` and the walk aborts
before the mount table is ever consulted. `/proc`, `/dev` and `/sys` work
because their parent is `/`.

**Root cause.** `Vfs::mount_with_options` validated only that the path was
absolute and not already mounted. It never checked reachability, so it happily
registered a mount that consumed an `fs_id`, printed a success line and
appeared in `/proc/mounts` while being addressable by nothing.

**Fix.** `mount_with_options` now stats the mount point's parent and refuses
the mount unless it is an existing directory (`NotADirectory`, or the stat's
own error). Only the *parent* is required, not the mount point itself: Linux
requires the mount point directory to exist, but our boot sequence mounts
`/proc`, `/dev` and `/sys` over a root memfs that has no such directories, and
longest-prefix matching makes the mount point itself reachable regardless.
Requiring the parent is the weakest condition that makes "registered" and
"reachable" mean the same thing. The check runs *before* `VFS.lock()` is taken,
since `stat` re-enters the VFS.

Overlay self-test 13 now creates `/mnt` before mounting.

**Also fixed: the test hid which step failed.** Test 13 used bare `?` on four
fallible calls, so the only diagnostic was
`Overlay filesystem self-test failed: NotFound` — naming neither the operation
nor the path. Each step now reports its name and error and unwinds the mount
and the scratch tree, which is what turned this from a guessing exercise into a
one-boot diagnosis.

**Bug class.** Same shape as the `pthread_join`-returns-`Ok(0)` defect fixed the
same day: an operation that cannot do what was asked reports success, and the
damage surfaces later somewhere unrelated. Worth auditing other registration
APIs that take a path and validate only its syntax.

---

### [A] TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE. The whole performance suite — baselines, targets, scorecard — is spawned and then killed mid-run on every boot test — 2026-08-14 — **FIXED** (all three options landed; see "Closed 2026-08-14" below)

**Where:** `kernel/src/main.rs` (`deferred_bench_task`, spawn site ~5505),
`kernel/src/bench.rs` (`run_all`, `score`, `SCORECARD`),
`scripts/boot-test.sh` (`WAIT_MARKER`, default `BOOT_OK`),
`bench/baselines.toml`.

**The shape of it.** Benchmarks run in a deferred low-priority kernel task that
prints `BENCH_OK` *after* `BOOT_OK`. That deferral is itself correct and well
reasoned — the comment explains it gets init to a prompt in ~1 s instead of
~20 s under TCG. The problem is the other half: the routine boot test waits for
`BOOT_OK` and tears QEMU down at once, so the bench task is killed before it
produces numbers.

**Evidence.** In the clean 26094-line KASAN boot
(`build/serial-kasan-pass.txt`), `[bench] === Kernel micro-benchmarks ===` is
line 26092 — the **second-to-last line in the file**. The task got just far
enough to print its own header before QEMU died. In an ordinary boot log the
header does not appear at all. Neither log contains a single benchmark result
or a `BENCH_OK`.

**Why it matters.** This is the reason
`B-FAST-CPU-INDEX-FELL-BACK-TO-AN-APIC-MMIO-READ-ON-EVERY-ALLOC` shipped
unnoticed: CLAUDE.md requires benchmarking after any change to a
performance-critical subsystem, `page_alloc_free` has a recorded QEMU baseline
of 198 ns / 736 cycles to compare against, `score()` computes a pass/fail
verdict — and none of that machinery has executed in the harness. A suite that
is never run is worse than no suite, because its existence is taken as
coverage.

**Same class as `B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT`** (above): a check that
silently did not run while the boot reported PASSED. That one was fixed by
making the skip *loud*. The same principle applies here.

**Proper fix.** `scripts/boot-test.sh --bench` already exists and does the right
thing — it switches `WAIT_MARKER` to `BENCH_OK` and surfaces `ABOVE TARGET`
verdicts — it simply is not part of any routine gate. Options, in preference
order:

1. Make the *absence* of benchmark results loud rather than silent, mirroring
   the Path-Z fix: have the boot test note when it terminated with the bench
   task still pending, so "no numbers" is visible instead of assumed-fine.
2. Run `--bench` on a schedule rather than every boot (it roughly doubles the
   ~405 s cycle under TCG, which is why making it the unconditional default is
   unattractive), specifically after any change touching `mm/`, `sched/` or
   `ipc/`.
3. Record the scorecard to a file the harness can diff across runs, so a
   regression is a *comparison* rather than a threshold — thresholds as loose
   as these (1000 ns against a 198 ns baseline) would not have caught a 3-4x
   allocator regression anyway.

Note that (3) is the one that would actually have caught the bug that motivated
this entry: a 736 → ~2500 cycle regression still passes a 3700-cycle target.
The targets are sized against Linux, not against our own last-known-good.

**Progress 2026-08-14 — (3) is DONE; (1) and (2) remain open.**
*(Superseded later the same day: (1) and (2) are now done too — see
"Closed 2026-08-14" at the end of this entry.)*

`print_scorecard` now emits a machine-readable line for **every** entry, not
just the failures:

```text
[bench] SCORE <name> <measured_ns> <target_ns> <PASS|OVER>
```

`scripts/bench-history.py` parses those out of the serial log, appends a
JSON-lines record (timestamp, host, git commit, all measurements) to
`bench/history.jsonl`, and diffs the run against the previous record **from the
same host**, reporting anything that moved more than a threshold (default 25%)
plus benchmarks that appeared or vanished. `boot-test.sh::print_bench_results`
invokes it automatically, non-fatally.

Three things about the design are deliberate:

* **Passing entries are recorded too.** The old failure-only list was blind to
  precisely the bug that motivated this entry — a benchmark that doubles while
  still beating a Linux-sized target never appeared in the output at all.
* **Diffs are same-host only.** A different machine or QEMU build moves every
  number at once; reporting "no baseline" beats reporting a hardware difference
  as a regression.
* **Over-target is no longer phrased as a failure**, in the kernel output or in
  `boot-test.sh`. It is labelled reference. That follows directly from
  `TD-BENCH-OWNER-AB-BUDGET-WAS-AN-ABSOLUTE-CYCLE-COUNT`: under TCG the
  hardware targets are unreachable by construction, so treating them as
  verdicts trains the reader to ignore the whole suite — which is how a real
  regression hid in it.

`boot-test.sh` previously advised "compare against prior runs rather than
treating this as a hard regression" while nothing stored prior runs, making the
advice unfollowable. It is now followable.

Still open: (1) making the *absence* of benchmark results loud on a routine
non-`--bench` boot, and (2) deciding when `--bench` runs, since it roughly
doubles the boot cycle under TCG.

**Closed 2026-08-14 — (1) and (2) landed together, because (2) turned out to
be answerable by (1) rather than by a schedule.**

(1) is `report_bench_absence()` in `scripts/boot-test.sh`, called on both PASS
paths whenever `--bench` was *not* given. It prints a `=== NO BENCHMARK
RESULTS THIS RUN ===` block and never changes the exit code — a routine boot
is *allowed* to skip the suite. The point is only that `PASSED` must not be
readable as "performance was checked". It distinguishes the two states the log
can be in: the deferred task started and was killed at `BOOT_OK`, or it never
reached its first result.

(2) as written — "run `--bench` on a schedule … after any change touching
`mm/`, `sched/` or `ipc/`" — assumes a scheduler that does not exist here, and
assumes someone remembers the rule at the right moment. That is the same
failure mode as the original bug: coverage that depends on being remembered.
Since `bench-history.py` already stamps every recorded run with its git
commit, the harness can just *compute* the answer instead:

```sh
git diff --name-only <last_benchmarked_commit> HEAD -- kernel/src/{mm,sched,ipc,syscall} kernel/src/smp.rs
```

Non-empty ⇒ this boot contains unbenchmarked changes to code CLAUDE.md
requires benchmarking, and the block escalates to `!! Performance-critical
code changed since the last benchmarked commit`, naming the files. Empty ⇒ it
says skipping the suite is reasonable here. So the nag is targeted and
automatic rather than periodic, and it cannot be forgotten.

Degenerate cases are handled explicitly rather than by silence, since the
whole entry is about silence: no `history.jsonl` yet ⇒ "no baseline for this
host"; a recorded commit absent from the repo (rebased away, or not fetched)
⇒ say so rather than diffing against nothing and reporting a false all-clear.

**Verified** by exercising all six branches against real and synthetic serial
logs: suite-started-then-killed, suite-never-started, perf-critical files
changed, nothing changed, missing history, unknown commit. On the current tree
it correctly reports `kernel/src/syscall/{handlers,number}.rs` as changed
since `bf26aabdb`.

**Not fixed by this, and deliberately so:** the kernel still spawns the
deferred bench task on every boot and still has it killed at `BOOT_OK`. That
wasted work is cheap (the task prints a header and dies), and suppressing the
spawn on non-`--bench` boots would need a kernel cmdline flag for no real
gain. The defect was never the wasted work — it was that nobody could tell it
had happened.

---

### B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT. 26 Path-Z self-test rungs (every `tcc` rung, Parts 35–60) have been no-opping on every boot while the boot test reported PASSED — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/proc/spawn.rs`, `kernel/src/main.rs`, `scripts/boot-test.sh`)

**Verified fixed 2026-08-13.** `rootfs.ext4` was rebuilt with tinycc present and a
full boot test confirms all 26 tcc rungs now run and pass under CR4.SMAP
(compile → glibc-link → ld.so → ring 3, including signal, setjmp, varargs, SSE,
struct-by-value, x87 long double, TLS and ctor/dtor), ending in
`[spawn] Path-Z prerequisites: complete — 0 rungs skipped`. A boot that *does*
lose coverage now prints one `SKIP:` line per rung naming the missing file, a
nonzero summary count, and a `=== PATH-Z COVERAGE INCOMPLETE ===` block from
`boot-test.sh` — so the silence that caused this can no longer recur.

**Symptom.** `rootfs.ext4` contains no TinyCC:

```
$ grep -c TinyCC rootfs.ext4
0
```

and the serial log of a *passing* boot ends its Path-Z sequence at Part 34:

```
[spawn] Running REAL GNU make (ring 3, Path Z) test...
[spawn]   REAL GNU make (...): OK
[hunt] Path-Z checkpoint: ...          <- straight on to the next subsystem
```

Parts 35–60 — every rung that compiles C on the target (`self_test_linux_
real_glibc_cc`, `..._cc_hosted`, `..._cc_hosted_stdio`, `..._cc_separate`,
`..._make_cc`, and the twenty-odd codegen rungs after them: signal, setjmp,
varargs, SSE, struct-by-value, x87 long double, bitfields, indirect call,
computed goto, unions, function-local statics, VLAs, inline asm, `_Atomic`,
statement expressions, `_Generic`, switch jump tables, TLS, ctor/dtor) — print
**nothing at all** and return `Ok(())`.

**Root cause.** Every Path-Z rung opens with a prerequisite guard of the shape

```rust
// kernel/src/proc/spawn.rs:25781
if !crate::fs::Vfs::exists(SRC_TCC) {
    return Ok(());
}
```

There are **40** such guards in `spawn.rs`. They exist for a good reason — the
rungs need artifacts staged into `rootfs.ext4` by `scripts/create-ext4-rootfs.sh`
and the image is optional — but the skip is *silent*: no marker, no counter, no
difference in the boot log between "ran and passed" and "never ran". A rung that
never runs is indistinguishable from a rung that passed, so `boot-test.sh` exits
0 either way.

**Why the artifact went missing.** `create-ext4-rootfs.sh` takes tcc from `PATH`
or from a cached source build at `/tmp/tccinstall/bin/tcc` (tcc is not on a
default Ubuntu install and `apt install tcc` needs root). `/tmp` was cleared at
some point on the WSL build host, so a later rootfs rebuild found neither, hit
the script's `else` branch, and produced an image without `/bin/tcc`. The script
*does* warn at build time — but that warning scrolled past months ago and
nothing downstream ever noticed.

**Consequence beyond the lost coverage.** This silently invalidated the planned
Q43 option-E experiment. B-KNULLJUMP is only ever observed during the
**tcc-signal Path-Z** rung; a 250-boot soak of the current image would have
sampled a population in which the trigger never executes, and a clean result
would have been read — wrongly — as evidence the `B-NO-CLD-ON-INTERRUPT-ENTRY`
fix landed. The soak was caught before it started only because the archived
soak serial log was grepped for `tcc` and came back empty.

**Lesson (the same one as `B-EXCEPTION-FRAME-WRITTEN-TO-ATTACKER-CHOSEN-RSP`,
from the other direction).** There, enforcement that was *off* hid a bug until
it was turned on. Here, a test that was *skipped* hid its own absence. Both are
the same failure of instrumentation: a green result that carries no information.
**A skip must be at least as loud as a failure**, because a silent skip is
strictly worse than a failure — a failure gets investigated.

**Fix.**
1. Route every prerequisite guard through one helper that logs a `[spawn] SKIP:`
   line naming the rung and the missing path, and bumps a counter.
2. Print a summary line after the Path-Z sequence, and make `boot-test.sh`
   surface a nonzero skip count.
3. Rebuild `rootfs.ext4` with `/bin/tcc` staged.

### B-EXCEPTION-FRAME-WRITTEN-TO-ATTACKER-CHOSEN-RSP. Arbitrary kernel write from any process with an exception handler — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/idt.rs`)

**Symptom that found it.** Enabling CR4.SMAP wedged the boot at the
`spawn-test-seh-exit` self-test with a fatal kernel #PF: write to
`0x7ffffffeff50` (a *user* stack address) from `memcpy` inside
`idt::try_dispatch_user_exception`, error code `0x3` = present + write +
supervisor. That is the textbook SMAP violation signature: the page is there
and writable, the kernel just is not allowed to touch it outside a STAC window.

**The SMAP part.** `try_dispatch_user_exception` built the `ExceptionContext` on
the user stack with a raw `core::ptr::write(ctx_addr as *mut ExceptionContext,
ctx)` plus a second write for the null return address. Both are supervisor
writes to a user page.

This path was missed by the sweep done for
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE` (below) for a reason
worth recording: that sweep enumerated the files that take a user address **out
of a syscall argument**. This one takes it out of the *ring-3 interrupt frame's
RSP*. It matched neither the greps nor the mental model, and only the hardware
found it. **Enabling the enforcement is the audit** — a "we converted everything"
claim is not checkable by reading, and this is the third time on this entry that
a syntax-shaped search missed a genuine user access (see also `futex.rs`'s
`&*(addr as *const AtomicU32)`).

**The much worse part, which has nothing to do with SMAP.** `rsp` is read out of
the interrupt frame the CPU pushed for a **ring-3** fault, so it is whatever the
faulting thread had in RSP — entirely attacker-chosen. `ctx_addr` is derived from
it by subtraction and alignment, with no check that the result is a user address.
CR3 is still the process's PML4, which maps the kernel. So:

1. An unprivileged process calls `SYS_SET_EXCEPTION_HANDLER` (no capability
   required).
2. It sets RSP to any kernel address and executes a faulting instruction.
3. The kernel writes a 168-byte `ExceptionContext` there — 15 of whose fields
   are that process's own general-purpose registers, i.e. fully chosen content.

That is an arbitrary kernel write with attacker-controlled data, available to
any process, with no capability and no privilege. It long predates the SMAP
work; SMAP is simply what made the code get read closely.

**Fix.** Both stores now go through `crate::mm::user::write_user_value`, which
validates the destination is below `USER_SPACE_END` and writable, faults in a
demand-paged page, breaks CoW (a freshly-forked process's stack is CoW, and the
kernel cannot rely on its own write triggering the CoW fault handler), and
brackets the copy in STAC/CLAC. On failure it returns `false`, so the caller
kills the process exactly as if no handler had been registered — the right
answer for an unwritable stack too, since there is nowhere to put the frame.
Linux reaches the same outcome via `force_sigsegv`.

**Why the STAC window could not have been opened by hand here.**
`validate_user_write` may fault a page in and may break CoW; both can block. An
AC window held across a reschedule leaves `AC = 1` in the task's saved RFLAGS,
disabling SMAP for that task *and* for the scheduler. The window has to live
inside `mm::user`, around the copy alone.

**Lesson.** Any kernel write whose address derives from a *saved ring-3
register* is as untrusted as one derived from a syscall argument. The
interrupt-frame fields (`rsp`, `rip`, `rflags`, and every `SavedRegisters` GPR)
are user input. Grep for uses of `frame.rsp` / `(*frame).rsp` before trusting
one as a destination.

### B-FRAME-REWRITING-RETURNS-INSTALLED-UNSANITISED-USER-STATE. Every syscall that rewrites the return frame took RIP/RSP/RFLAGS from userspace unchecked — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/syscall/entry.rs`, `kernel/src/syscall/handlers.rs`, `kernel/src/syscall/linux.rs`)

**Context.** Three syscalls do not return to their caller — they overwrite the
saved return frame so the SYSRET path resumes somewhere else entirely:

| Syscall | File | Source of the new state |
|---|---|---|
| `sys_exception_return_with_frame` | `handlers.rs` | user `ExceptionContext` |
| `sys_signal_return_with_frame` | `handlers.rs` | user `SignalContext` |
| `linux_rt_sigreturn` (Linux ABI) | `linux.rs` | user `ucontext.uc_mcontext` |

All three read RIP, RSP and RFLAGS out of a userspace structure and stored them
straight into `frame.user_rip` / `frame.user_rsp` / `frame.user_rflags`. Nothing
between the copy-in and `sysretq` looked at the values. This was found while
converting the two `handlers.rs` entries away from raw user pointers for
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE` (below) — the pointer
conversion is unrelated, but writing an honest justification comment for it
required checking what *did* sanitise the frame, and the answer was "nothing".

**Bug 1 — non-canonical RIP faults at CPL 0 (CVE-2012-0217 shape).** `sysretq`
loads RIP from RCX *while still at ring 0* and only then drops privilege. If
RCX is non-canonical, the #GP is raised in ring 0, not ring 3. This kernel makes
that strictly worse than the classic bug: `syscall_entry_stub` in `entry.rs`
does `mov rsp, gs:[8]` and `swapgs` **before** `sysretq`, so at the moment of
the fault RSP is attacker-influenced and the GS base is the user's. The #GP
handler then runs at kernel privilege on a stack and per-CPU base the attacker
chose. A userspace thread could trigger this with a one-line `ExceptionContext`
whose `rip` was `0xFFFF_8000_0000_0000`.

**Bug 2 — unsanitised RFLAGS.** The restored RFLAGS went to ring 3 verbatim, so
a caller could set:

- **IOPL = 3** — direct `in`/`out` to every I/O port from an unprivileged
  process. That is a full privilege escalation on its own: PCI config space,
  the PS/2 controller, the PIT, the CMOS/RTC, ATA PIO.
- **NT** (nested task) — corrupts a subsequent `iret` into a task switch.
- **VM** (virtual-8086) — puts the CPU in a mode the rest of the kernel has no
  handling for.
- **VIF/VIP** — meaningless without VME but sets up confusing state.
- **IF cleared** — returns to ring 3 with interrupts disabled, which wedges that
  CPU until something re-enables them (nothing does).

`linux.rs` was the one path that *had* a mask (`SIGRETURN_RFLAGS_USER_MASK`),
but it was a local constant covering only that function, and it did not check
RIP/RSP at all.

**Bug 3 — the registration side accepted kernel addresses.** Even with the
return paths fixed, `sys_signal_register`, `sys_set_exception_handler` and
Linux `sys_rt_sigaction` would accept a *kernel* address as the handler /
trampoline. Delivery builds a frame targeting that address, so the check has to
exist at registration too or the same kernel RIP arrives by a different route.

**Fix.** One shared policy in `kernel/src/syscall/entry.rs`, applied at every
path rather than reimplemented per-caller:

```rust
pub const USER_RFLAGS_MASK: u64   = 0x0024_0DD5; // CF PF AF ZF SF TF DF OF AC ID
pub const USER_RFLAGS_FORCED: u64 = 0x0000_0202; // IF + reserved bit 1

pub const fn sanitize_user_rflags(raw: u64) -> u64 {
    (raw & USER_RFLAGS_MASK) | USER_RFLAGS_FORCED
}

pub fn user_return_state_ok(rip: u64, rsp: u64) -> bool {
    rip < crate::mm::page_table::USER_SPACE_END && rsp < crate::mm::page_table::USER_SPACE_END
}
```

`user_return_state_ok` is deliberately stronger than a canonicality test:
`rip < 0x0000_8000_0000_0000` rejects the whole upper half, so a canonical
*kernel* address is refused as well as a non-canonical one. All three return
paths now reject a failing pair with `EFAULT`/`InvalidAddress` **before**
touching the frame, and pass RFLAGS through `sanitize_user_rflags`.
`linux.rs`'s `SIGRETURN_RFLAGS_USER_MASK` was deleted and its
`SIGRETURN_RFLAGS_FORCED` re-pointed at `entry::USER_RFLAGS_FORCED` so the
kernel-built signal frame and the user-supplied one cannot drift apart. The
three registration syscalls now reject a handler `>= USER_SPACE_END` up front
(0 still means unregister / `SIG_DFL` / `SIG_IGN`).

**Why reject rather than Linux's lazy approach.** Linux lets a bad
`rt_sigaction` handler through and only kills the process when delivery
faults (`force_sigsegv`). Rejecting at registration gives the caller a
diagnosable `EFAULT` at the point of the mistake instead of an unexplained
death later, and it means the delivery path has one fewer failure mode to
unwind. Nothing in-tree registers a kernel address, so there is no
compatibility cost.

### B-IO-RING-SUBMISSION-PATH-WAS-UNGATED-AND-UNVALIDATED. Four separate ring-3-reachable holes in `io_ring` — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/ipc/io_ring.rs`)

**Context.** `SYS_IO_RING_SETUP` (260) and `SYS_IO_RING_ENTER` (261) are
registered in the dispatch table with **no capability gate**, so every item
below was reachable from any unprivileged process. There are as yet no
userspace consumers — the only in-tree callers are `completion.rs`'s
`test_io_completion` and `bench.rs`'s `bench_io_ring_nop`, both of which submit
`IO_OP_NOP` with `addr: 0` — so nothing legitimate depended on the old
behaviour. That absence of users is also why this sat unnoticed: the subsystem
is fully wired to ring 3 but exercised only by two self-tests that never touch
the dangerous paths.

**(1) `sqe.addr` was dereferenced with no validation whatsoever.** Every
buffer-moving opcode did `sqe.addr as *const u8` / `as *mut u8` and built a
slice over it:

```rust
// SAFETY: Caller guarantees ptr is valid for len bytes.
let bytes = unsafe { core::slice::from_raw_parts(ptr, len.min(4096)) };
```

The "caller" is userspace filling a shared submission queue. No
`validate_user_read`/`validate_user_write` appeared anywhere in the file. This
gave, from an unprivileged process:

- **Kernel memory disclosure.** `IO_OP_CONSOLE_WRITE` with a kernel address
  printed up to 4 KiB of kernel memory to the console;
  `IO_OP_CHANNEL_SEND` / `IO_OP_PIPE_WRITE` / `IO_OP_FH_WRITE` /
  `IO_OP_FS_WRITE` exfiltrated it to a peer process or to a file.
- **Arbitrary kernel write.** `IO_OP_CHANNEL_RECV`, `IO_OP_PIPE_READ`,
  `IO_OP_FS_READ`, `IO_OP_FH_READ` and `IO_OP_FH_PREAD` copied
  attacker-influenced bytes to an address the attacker named.
- **Unbounded kernel allocation.** `exec_channel_send` and `exec_fs_write` took
  `len` straight from the SQE with no cap at all.

**(2) `execute_sqe` ran with the global `RING_TABLE` spinlock held.**
`enter()` held the lock across the whole processing loop. `IO_OP_SLEEP` parks
for up to 60 seconds and `IO_OP_TIMEOUT` for up to 10, and every buffer opcode
can block on filesystem I/O or a demand-paging fault. One process could
therefore hold a global kernel spinlock for a minute at a time, wedging every
other thread touching *any* ring.

**(3) No entry point checked ownership.** `owner_task` was stored and never
read. Handles come from `NEXT_RING_ID.fetch_add(1)` starting at 1, so they are
trivially guessable. Consequences:

- `destroy(other_ring)` freed the physical frames of another process's ring
  while that process still had them mapped — a **cross-process use-after-free
  of physical memory**, and the freed frames are then handed to somebody else.
- `set_cp(other_ring, 0)` silently detached a victim's ring from its completion
  port, so the victim's event loop would never wake again.
- `enter(other_ring)` executed a victim's queued SQEs in the *attacker's*
  address space and with the attacker's handle table.

**(4) `SYS_IO_RING_SETUP` returned an HHDM address to userspace.**
`io_ring::setup` returns `phys_frames[0] + hhdm`, a kernel direct-map pointer,
and the handler passed it back in `value2`. Ring 3 could not dereference it,
but simply *knowing* it discloses the direct-map base and so defeats kernel
address-space randomisation for any subsequent attack. The frames were also
never mapped into the caller at all, so the documented interface ("maps into
user space, returns the virtual address of the `IoRingHeader`") did not work.

**Fix.**

1. Every payload goes through `mm::user`. The length is a **rejection**
   threshold for the all-or-nothing opcodes (`CHANNEL_SEND`, `FS_WRITE`,
   `SERVICE_CONNECT` — where the CQE result is 0/error, so clamping would
   truncate the message/file/service name and report success) and a
   short-transfer **cap** only where the CQE result reports bytes consumed
   (`CONSOLE_WRITE`, `PIPE_*`, `FH_*`, `FS_READ`), which is the `write(2)`
   contract a caller already loops on.
2. `enter()` claims the ring with a `busy` flag under the lock, releases the
   lock, processes, then re-acquires to clear the claim. `busy` supplies the
   exclusion the lock used to: one entrant per ring, and `destroy` refuses
   (`WouldBlock`) while the processing loop is still reading SQEs out of those
   frames.
3. Ownership is by **process**, not task — the SQ/CQ are a mapping in one
   address space, so every thread of the owner is a legitimate driver and no
   thread outside it is. `enter`/`destroy`/`set_cp`/`attach_user_mapping` are
   owner-checked; `has_completions_ready` deliberately is not (it runs on
   whichever task polls a completion port and leaks only a bool), and says so
   in its doc comment.
4. `sys_io_ring_setup` maps the frames into the caller's address space
   read/write/no-execute with a per-frame `ref_inc` (the `sys_shm_map` model, so
   `destroy` dropping the ring's reference cannot pull memory out from under a
   live mapping), with full unwind on partial failure, and returns *that*
   address. `destroy` unmaps the recorded range first. `setup`'s doc comment now
   states in terms that the address it returns is HHDM and must not reach
   userspace.

**Also fixed in passing.** `exec_fh_pread`/`exec_fh_pwrite` restored the file
cursor with `let _ = seek(...)`, discarding the error. A failure there leaves
the handle parked at `offset + n`, silently corrupting every subsequent
sequential access — far worse than the pread itself failing. Both now report it.

**How it was found.** Working through
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE` below: `io_ring.rs`
was next on the file list after `handlers.rs`, and the first grep for
`from_raw_parts` over a user address turned up fifteen sites, none validated.

### B-NET-DIAGNOSTIC-HANDLERS-WROTE-TO-AN-UNVALIDATED-USER-POINTER. Five uncapability-gated syscalls were a write-what-where primitive — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/syscall/handlers.rs`)

**What.** `sys_tcp_list`, `sys_tcp_listener_list`, `sys_net_if_info`,
`sys_arp_table` and `sys_dns_cache_stats` each wrote their output records
straight through `args.arg0 as *mut u8`, having checked only that the argument
was non-zero:

```rust
if buf_ptr == 0 { return SyscallResult::err(KernelError::InvalidArgument); }
...
// SAFETY: buf_ptr is a userspace pointer validated by the caller;
// written < max_records ensures dst stays within the buffer.
let dst = (buf_ptr + written * RECORD_SIZE) as *mut u8;
unsafe { core::ptr::copy_nonoverlapping(record.as_ptr(), dst, RECORD_SIZE); }
```

Nothing validated it. `validate_user_write` was never called, and the SAFETY
comments asserted a precondition that no code established — "validated by the
caller" (there is no such caller; this is the syscall boundary), or in
`sys_dns_cache_stats`' case `buf_len >= STATS_SIZE`, which bounds the *length*
and says nothing about the *address*.

**Why it mattered.** None of the five requires a capability. So any process at
all could pass an arbitrary kernel virtual address and have the kernel write
attacker-influenced bytes over it: the TCP connection table (remote IPs and
ports the attacker chooses by opening connections), the interface config, the
ARP cache, or the DNS counters. That is a write-what-where primitive with
partial content control — enough to corrupt page tables, a task struct, or a
capability table. It was reachable from an unprivileged process with no
capability held.

`sys_net_route_list` and `sys_tcp_info`, in the same file and the same style,
*did* call `validate_user_write`, which is what made the omission easy to miss
on a read-through.

**Fix.** All five now pack their records into a kernel-owned buffer and deliver
it with a single `copy_to_user`, which performs the validation the comments
assumed (and brackets the store with STAC/CLAC for SMAP). Sizing the scratch
buffer by the number of records that actually exist, rather than by the
caller's advertised `buf_len`, additionally stops a caller demanding an
arbitrary kernel allocation.

**How it was found.** Not by looking for it — by working through
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE` below and reading
every SAFETY comment on a user-pointer access to check whether the invariant it
claimed actually held. Five did not. **The lesson worth keeping: a SAFETY
comment that names a precondition without pointing at the code that establishes
it is not evidence, and in this file it was wrong five times out of five.**

---

### B-FS-HANDLE-PREAD-PWRITE-ARE-NOT-ATOMIC — 2026-08-13 — ✅ FIXED 2026-08-13 (`kernel/src/ipc/io_ring.rs`)

**Fix.** `exec_fh_pread` and `exec_fh_pwrite` now call
`fs::handle::read_at` / `fs::handle::write_at`. The seek sandwich is gone from
the kernel entirely (`grep -rn 'SeekFrom::Current(0)' kernel/` finds only the
handle self-test that legitimately *queries* the cursor).

The proper fix turned out to be **already three quarters built**: `read_at` and
`write_at` had existed in `kernel/src/fs/handle.rs` since the `pread64`/
`pwrite64` Linux-ABI work — they take `OPEN_FILES.lock()` once and index the
file without ever consulting `file.offset`, which is exactly the contract this
entry asked for. Every *syscall* path already used them
(`linux.rs:22995/24230/41027/41073`, `pcb.rs`'s mmap fault fill); only the
io_ring opcodes still had the hand-rolled emulation. So this was not missing
infrastructure, it was **one caller that never got migrated** — worth
remembering the next time an entry's "proper fix" reads like a large project:
check whether the primitive already exists under a different caller.

Two behaviours improved as a side effect:

- a `pread` on a *directory* handle now fails `IsADirectory` instead of
  seeking and then failing somewhere less specific;
- `pwrite` now ignores `O_APPEND`, which is what POSIX and Linux both
  specify ("the offset argument shall be used"). The old
  `seek`-then-`handle::write` path let an `O_APPEND` handle silently redirect
  the write to end-of-file, discarding the caller's offset.

**Regression test.** `test_fh_positioned_io_leaves_the_cursor_alone` in
`io_ring.rs` (run from both `self_test` and the post-mount `self_test_fh`).
It writes a 26-byte self-identifying file (`A`..`Z`), reads 4 bytes
sequentially, then interleaves a `PREAD` at offset 20 and a `PWRITE` at offset
10, asserting after each that `seek(Current(0))` is still 4 — and finally that
the next sequential read yields `EFGH`, i.e. the stream never noticed the
positioned I/O at all. That last assertion is precisely what the seek sandwich
could not offer a concurrent peer.

**The original report follows.**

**What.** `IO_OP_FH_PREAD` and `IO_OP_FH_PWRITE` are emulated as
`seek(Current(0))` → `seek(Start(offset))` → `read`/`write` →
`seek(Start(saved))`. The whole sequence is not atomic with respect to the file
handle's cursor.

**Impact.** A file handle can be shared between threads (and, after
`SYS_PROCESS_SET_EXEC_FDS`, between processes). If a peer touches the same
handle inside that window, the results interleave arbitrarily:

- the peer's `read` starts from `offset` instead of its own position;
- the restore at the end clobbers a position the peer legitimately advanced;
- two concurrent preads on one handle can each restore the *other's* saved
  position, leaving the cursor somewhere neither of them was.

The whole point of `pread`/`pwrite` (POSIX) is to be a positioned I/O that does
**not** disturb the cursor and is safe to issue concurrently on a shared
descriptor. This implementation provides neither guarantee.

**Repro sketch.** Two threads sharing one `fh`: thread A submits
`IO_OP_FH_PREAD` at offset 0 in a loop; thread B does sequential
`IO_OP_FH_READ`s. B's stream will show repeats/gaps as A's restore lands
between B's reads.

**Proper fix.** Give `fs::handle` real positioned operations —
`read_at(fh, offset, buf)` / `write_at(fh, offset, buf)` — that take the
handle's lock once and index the file without touching the stored cursor at
all, and have the io_ring opcodes (and any future `pread`/`pwrite` syscalls)
call those. The seek-sandwich should not exist anywhere. Until then the io_ring
opcodes at least report the restore failure instead of discarding it.

### D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE — 2026-08-13 — TECH DEBT (blocked enabling SMAP) — ✅ FIXED 2026-08-13 (CR4.SMAP is on)

**What.** Roughly 100 syscall handlers in `kernel/src/syscall/handlers.rs`,
`kernel/src/syscall/linux.rs` and `kernel/src/ipc/io_ring.rs` follow this shape:

```rust
if let Err(e) = crate::mm::user::validate_user_write(args.arg1, buf_cap) { ... }
// SAFETY: Buffer validated above — in user space, mapped, writable.
let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, buf_cap) };
match pipe::read(handle, buf) { ... }
```

That is, they validate a user pointer and then construct a Rust slice *over the
user virtual address itself* and pass it into arbitrary kernel code. `mm::user`
is not involved beyond validation.

**Why this blocks SMAP.** With CR4.SMAP set, every one of those accesses faults:
they are supervisor-mode reads/writes of user pages performed outside any
STAC/CLAC window. This is what `smep_smap::USER_ACCESSES_ANNOTATED = false`
stood for, and it is why `smap_enable_blocker()` kept refusing to set the bit
even after `B-AC-INHERITED-AT-KERNEL-ENTRY` was fixed.

**Why the obvious fix is wrong.** Wrapping each site in `stac()`/`clac()` would
make it *compile and boot*, and would be a serious bug. `pipe::read` blocks: it
registers a waiter and reschedules with the slice still live. So a `stac()` held
across it would (a) leave AC = 1 in the task's saved RFLAGS, so SMAP stays
disabled for that task across the context switch and the scheduler itself runs
with the override on, and (b) hold the window open for an unbounded time. The
STAC window must stay inside a single non-blocking copy, which is exactly what
`mm::user::copy_{from,to}_user` already does.

**The independent bug underneath.** Even with SMAP off, holding a raw user slice
across a blocking call is a TOCTOU use-after-free: another thread in the same
process can `munmap`/`mremap` the range while the caller sleeps, and the kernel
then writes through a stale user mapping — into whatever now owns that physical
page. SMAP would merely convert this from silent corruption into a fault. So
this is worth fixing on its own merits, independent of SMAP.

**Proper fix.** Handlers must not hand user virtual addresses to kernel
subsystems. Either:

1. **Bounce through a kernel buffer** — `copy_from_user` into kernel memory,
   call the subsystem, `copy_to_user` the result back. Correct everywhere,
   costs a copy on paths that currently have none.
2. **Use the `_as` accessors** (`mm::user::copy_{from,to}_user_as`) which
   resolve the user VA to a physical frame and access it through the HHDM.
   Those are supervisor mappings, so SMAP does not apply and no STAC window is
   needed at all — but the frame must be pinned for the duration, which is the
   part that does not exist yet.

(2) is the better end state for large transfers; (1) is right for the many
small ones. Either way this is a mechanical but wide refactor across ~100 call
sites, and it wants a pinning primitive before (2) is available.

**Where.** `kernel/src/syscall/handlers.rs` (~60 sites — grep
`from_raw_parts`), `kernel/src/syscall/linux.rs`, `kernel/src/ipc/io_ring.rs`;
gate at `kernel/src/smep_smap.rs::USER_ACCESSES_ANNOTATED`.

**Approach taken.** Option (1), the bounce, everywhere. Option (2) still wants
a frame-pinning primitive that does not exist; revisit it for the large
transfers once one does. Four shapes recur:

- **write** — `read_user_vec(ptr, len, MAX)` into a kernel `Vec`, then call the
  subsystem with `&data`.
- **read** — `with_user_out_buf(ptr, cap, MAX, |buf| subsystem_read(buf))`.
- **recv-then-copy** — keep the up-front `validate_user_write` so a bad
  destination costs the caller an error rather than a *dequeued* message with
  nowhere to go, then `copy_to_user`, which re-validates at the moment of the
  store. The second check is the one that matters: the dequeue blocks.
- **record packing** — fill a kernel buffer sized by what the kernel will
  actually emit (never by the caller's advertised capacity), then one
  `copy_to_user` for the batch. Storing records one at a time meant a fault
  partway through left the caller with a partial answer it could not detect.

**Progress (2026-08-13).** `handlers.rs` is done. Verified by grep, not by
recollection — the first time this entry claimed "done" the grep immediately
disproved it, which is why the check is recorded here:

```
grep -nE 'as \*const u8|as \*mut u8|as \*mut u64|as \*mut u32|as \*mut i32|copy_nonoverlapping|ptr::write|write_bytes|read_volatile|write_volatile|from_raw_parts' kernel/src/syscall/handlers.rs
```

now yields six hits, all benign: five are prose inside comments describing the
code that *used* to be there, and `handlers.rs:3655` takes a slice over a
`SpawnArgsHeader` living on the **kernel** stack, which is not a user access at
all. So: no `from_raw_parts` over a user address, no
`core::ptr::write`/`copy_nonoverlapping` through a user pointer, and no
raw-pointer locals derived from a syscall argument.

The last batch was the six net handlers, which all *did* validate first and so
were easy to skim past — validation is necessary but it is not the property
being restored here. Four inbound record readers (`sys_net_if_config`,
`sys_net_route_add`, `sys_net_route_del`, `sys_net_fw_add_rule`) became
`read_user_value::<[u8; REC_SIZE]>` — a `[u8; N]` has alignment 1, so the
fixed-size ABI record decodes through the bounce with no cast and no indexing.
`sys_tcp_info` became a single `copy_to_user`. `sys_net_route_list` was the only
one needing real restructuring: it stored records one at a time through
`buf_ptr as *mut u8`, so a fault on record 5 of 9 left the caller holding a
partial table *and* a return value claiming all nine — now packed in the kernel
and delivered as one copy, with the count reported from what was actually
packed.

`kernel/src/ipc/io_ring.rs` is done too, and was a different animal: not one of
its fifteen user accesses validated *anything*, so the conversion was a security
fix rather than a SMAP preparation — see
`B-IO-RING-SUBMISSION-PATH-WAS-UNGATED-AND-UNVALIDATED` above, which also covers
the three unrelated holes reading the file closely turned up (blocking with a
global spinlock held, no ownership checks, an HHDM address published to ring 3).
The lesson generalises: the files still to convert should be read as *audits*,
not as find-and-replace.

`kernel/src/syscall/linux.rs` and `kernel/src/drm/syscall.rs` are done as of
2026-08-13. `linux.rs` was much smaller than the raw grep suggested — of 23
hits, most were HHDM addresses in self-test code or prose inside comments, and
only eight were genuine user accesses. They fell into three groups:

- **wait/rusage** (`sys_waitid`, `sys_wait4`). Every one of these carried a
  SAFETY comment of the form *"validated as a writable user range before the
  wait began and the address space has not changed"* — wrong twice over, since
  the wait is exactly what blocks and a peer thread can `munmap` the range
  while the caller sleeps. Now `write_user_value` / a shared
  `clear_user_rusage`. The encoder was split out of `write_waitid_siginfo` as a
  pure `waitid_siginfo` so the byte-layout self-test — which fed it a *kernel*
  stack buffer — no longer drives the user-delivery path at all.
- **`rt_sigreturn`** read the whole `LinuxUcontext` off the user stack with
  `read_unaligned`; now `read_user_value::<LinuxUcontext>`, which also lands it
  in an aligned kernel local so the user-side alignment stops mattering.
- **`emit_linux_rt_frame`** wrote `pretcode`, `ucontext` and `siginfo` as three
  separate stores straight onto the user stack. Now packed into one
  `RT_SIGFRAME_SIZE` kernel array (the layout is contiguous by construction)
  and delivered with a single `copy_to_user`. This one needed care: the write
  can now *fail*, where before it could only fault, and by that point
  `take_saved_sigmask` has already consumed the pending `sigsuspend` mask — so
  the failure path puts it back, keeping the documented contract that a `None`
  return means nothing happened and the caller may retry.

`drm/syscall.rs` had a single site, and it was the worst-annotated one in the
tree: `sys_drm_atomic_commit` built a slice over the user buffer with *no
validation whatsoever* — `"SAFETY: The caller is responsible for passing a
valid buffer. In the current kernel-mode testing setup, all addresses are
valid."` — and then parsed record counts out of it. Besides the missing
validation that is a double-fetch: the counts and the records were read from a
live user mapping the submitting process can rewrite between reads. Now
`read_user_vec` with a 64 KiB cap.

`kernel/src/ipc/futex.rs` was **not** on the original file list because it uses
none of the grep's patterns: it casts the user address to `*const AtomicU32`
instead. Fourteen sites, and the one place where the bounce is *not* the right
answer, so it needed its own primitive — see
`D-FUTEX-ATOMICS-OPERATE-DIRECTLY-ON-USER-WORDS` below (now fixed).

A final sweep after that — grepping the *whole* kernel for
`from_raw_parts`/`as *const`/`as *mut`/`unsafe { &*(` and triaging each hit as
HHDM, kernel-local or user address — turned up three more genuinely-unconverted
user accesses, all in `handlers.rs`:

- **`sys_cp_wait` / `sys_cp_try_wait`** delivered completion events by storing
  them one 24-byte record at a time through `args.arg1 as *mut CpEventRaw`,
  after a `validate_user_write` that ran *before* `completion::wait` blocked.
  Now packed into a kernel `Vec` and delivered with a single
  `write_user_items`, which makes delivery all-or-nothing as well.
- **`sys_exception_return_with_frame` / `sys_signal_return_with_frame`** read
  the saved register context field-by-field through `&*(frame.arg0 as *const
  ExceptionContext)` — a double-fetch as well as an unbracketed access. Now
  `read_user_value`. Reading these two closely is also what surfaced
  `B-FRAME-REWRITING-RETURNS-INSTALLED-UNSANITISED-USER-STATE` (above).

**Done — CR4.SMAP is on (2026-08-13).** `USER_ACCESSES_ANNOTATED` is `true`,
`smap_enable_blocker()` returns `None`, and a boot under
`-cpu qemu64,+smep,+smap,+umip` reaches BOOT_OK (135 s) with CR4 = `0x300e20`
and no self-test failures.

The very first boot with the bit set did **not** get that far, and the failure
is the most useful result of this whole entry: a fatal kernel #PF writing to a
*user* stack address from `idt::try_dispatch_user_exception`, which builds the
SEH `ExceptionContext` on the user stack. That site was missed because every
sweep above enumerated code taking a user address **out of a syscall argument**;
this one takes it out of the *ring-3 interrupt frame's RSP*. Worse, the address
was never checked against `USER_SPACE_END` at all, making it an arbitrary kernel
write for any process that had registered an exception handler — see
`B-EXCEPTION-FRAME-WRITTEN-TO-ATTACKER-CHOSEN-RSP` above.

So the closing lesson of this entry, which cost three separate "surely we're
done now" moments to learn: **a grep proves nothing, and neither does a careful
reading; only turning the enforcement on is the audit.** Anything derived from a
saved ring-3 register — an interrupt frame's `rsp`/`rip`, any `SavedRegisters`
GPR — is user input exactly as much as a syscall argument is.

The gate constant stays in `smep_smap.rs` rather than being deleted: it is the
one documented place to turn SMAP back off if a fourth missed path turns up, and
the blocker string still reaches the serial log.

**Bugs found while doing it.** The refactor was worth far more than the SMAP
unblock — reading each site closely turned up a long list of live defects,
every one of which predates this work:

- **`B-NET-DIAGNOSTIC-HANDLERS-WROTE-TO-AN-UNVALIDATED-USER-POINTER`** (above)
  — five handlers, arbitrary kernel write, no capability required. The most
  serious find.
- **A kernel-panic vector in the xattr handlers.** They validated *one* byte
  and then scanned up to 256 looking for a NUL. An unterminated string at the
  end of a mapping walks the scan into an unmapped page, and the fault is taken
  in supervisor mode with no exception-table entry to recover from — so any
  process could panic the kernel with one unterminated buffer. Fixed by
  `mm::user::read_user_cstr`, which copies forward in page-bounded chunks.
- **Alignment UB in `mm::user::read_user`/`write_user`.** Typed
  `core::ptr::read`/`write` through a user-supplied pointer, behind a safety
  contract requiring the caller to "ensure it is properly aligned" — which a
  syscall ABI cannot enforce. Both deleted in favour of the byte-wise
  `read_user_value`/`write_user_value`.
- **Alignment UB in `sys_fs_metadata` for *every* caller.** `attributes` sits
  at ABI offset 58, two bytes past a `u16`, so `out_ptr.add(58) as *mut u32`
  was a misaligned typed store by construction — not merely reachable by a
  hostile caller.
- **A self-deadlock in `sys_log_read`.** `klog::read_logs` formats JSON-lines
  *while holding the log-ring spinlock*, straight into the caller's buffer, so
  a demand-paging fault on an untouched user page is taken with that lock held
  — and the fault path itself logs.
- **Six silent truncations**, each of which changed *which object* the syscall
  operated on rather than merely shortening a result: `sys_dns_resolve` (a
  clipped name resolves a different host), `sys_fs_symlink` (the link points
  elsewhere), `sys_fs_set_xattr` and `sys_fs_append` (clipped the data and
  returned *success* — silent corruption), `sys_cap_request` (clipped the
  human-facing reason string, so a request could be made to read as more
  innocuous than it is), and `ns_bind`/`ns_unbind`/`ns_hide` (a truncated
  prefix installs a sandbox rule over a *broader* subtree — failure in exactly
  the wrong direction). All now reject with `InvalidArgument`. The rule
  adopted: a length cap is legitimate **only** where the handler returns the
  number of bytes it consumed, making it a `write(2)`-style short write the
  caller loops on (`sys_debug_print`, console write, `readlink`).
- **`sys_fs_handle_path` reported the truncated length**, so a caller that
  filled its buffer exactly could not distinguish a fit from a clipped path.
  Now returns the full length — the `snprintf` contract.
- **Infallible allocations sized by a syscall argument** (`vec![0u8; buf_cap]`
  in `sys_fs_read` and elsewhere): on exhaustion these call the allocation
  error handler, i.e. a userspace-triggerable kernel abort. Now
  `mm::user::alloc_zeroed_vec`, which returns `OutOfMemory`.
- **`sys_getrandom` validated its destination *after* running the CSPRNG**, so
  a bad pointer consumed entropy for nothing.
- **Two false SAFETY comments on blocking paths** — the `waitpid` status write
  claimed "the address space cannot have changed since" on a path that sleeps,
  and `sys_process_crash_info` called a userspace buffer "always valid kernel
  memory".
- **Several handlers wrote multi-field records one field at a time**
  (`sys_net_stat`'s six counters, `sys_fs_watch_read`, `sys_process_get_args`,
  `sys_fs_journal_read`), so a fault partway through left the caller with a
  half-updated record — and in the `watch_read` and `get_args` cases the data
  had *already* been dequeued/consumed, so it was unrecoverably lost.

**Related.** `B-AC-INHERITED-AT-KERNEL-ENTRY` (fixed) was the *other*
prerequisite for SMAP. design-decisions §122 records why SMAP stays behind a
gate rather than being enabled optimistically.

---

### D-FUTEX-ATOMICS-OPERATE-DIRECTLY-ON-USER-WORDS — 2026-08-13 — TECH DEBT (blocked enabling SMAP) — ✅ FIXED 2026-08-13 (`kernel/src/ipc/futex.rs`, `kernel/src/mm/user.rs`)

**What.** Fourteen sites in `futex.rs` do

```rust
let atomic = unsafe { &*(addr as *const AtomicU32) };
```

over a *user* virtual address and then load / CAS / `fetch_or` / `swap` through
it. Under CR4.SMAP every one of those is a supervisor access to a user page
outside a STAC window, so they all fault.

**Why this was missed.** It is not in
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE`'s file list, and the
grep that drove that entry —
`from_raw_parts|write_unaligned|core::ptr::write|copy_nonoverlapping|…` — does
not match any of them. `futex.rs` reaches user memory through a *reference
cast*, a shape none of the other files use. It surfaced only from a second
sweep looking for callers of `validate_user_write` outside the syscall files.
**The lesson: an "is it all converted?" grep proves nothing about code that
reaches user memory by a shape the grep does not know about. Enumerate the
files that *take user addresses* and read them, rather than enumerating the
syntax you expect to find.**

**Why the bounce is the wrong fix here — uniquely.** Everywhere else in that
entry the answer is copy-in / operate / copy-out. A futex word cannot be
handled that way: the whole primitive *is* the atomicity of the RMW against
concurrent userspace CAS. Copy-in, modify, copy-out is not atomic and would
reintroduce exactly the lost-update race the futex exists to prevent.

This is therefore the one legitimate use of `stac()`/`clac()` in the kernel:
the window brackets a **single non-blocking atomic instruction**, so none of
the objections that rule it out for the handler sites apply — nothing blocks
inside it, so AC cannot leak into a saved RFLAGS across a reschedule, and the
window is a couple of cycles rather than unbounded. It is what Linux does
(`futex_atomic_cmpxchg_inatomic` and friends, `arch/x86/include/asm/futex.h`,
each wrapped in `__uaccess_begin()`/`__uaccess_end()`).

**Proper fix.** Add per-operation accessors to `mm::user` — `user_atomic_load_u32`,
`user_atomic_cas_u32`, `user_atomic_rmw_u32` — each of which validates the
address, opens the window, performs *one* atomic operation, and closes it.
Convert all fourteen sites to those. Critically, several sites currently hold
the `&AtomicU32` reference across code that blocks (`futex_lock_pi` at ~1492,
the requeue-PI paths at ~2272/~2453); those must become repeated per-operation
calls, with any CAS retry loop running *outside* the window, one instruction
per iteration. That is a correctness improvement independent of SMAP for the
same reason as the rest of the entry above: the reference is a raw user pointer
held across a sleep, so a peer thread's `munmap` turns it into a
use-after-free.

**Fixed as designed.** `mm::user` gained `user_atomic_load_u32`,
`user_atomic_store_u32`, `user_atomic_cas_u32` and
`user_atomic_rmw_u32(op, operand)` (`UserAtomicOp::{Set,Add,Or,AndN,Xor}`), each
validating the address and bracketing exactly one atomic instruction. All
fourteen sites converted; `futex.rs` now contains no raw-pointer cast at all
(`grep 'as \*const\|as \*mut\|from_raw_parts'` → no matches).

Three sites needed more than a mechanical swap, because making a previously
infallible store fallible introduces error paths that must not corrupt the
kernel-side bookkeeping:

- **`futex_unlock_pi`** — the ownership-transfer store now returns a
  `KernelResult`, but the handoff (`register_pi_owner` + `sched::wake`)
  completes *regardless*. The selected waiter has already been removed from the
  wait queue under the table lock, so bailing out on the store would park it
  forever on a lock nobody owns. `lock_pi_inner` consults the kernel ownership
  record, not the user word, so the handoff stays coherent; the error is
  reported to the unlocker, whose mapping is the one that vanished.
- **`futex_cmp_requeue_pi`** — the PI word is now read *before* each waiter is
  dequeued, so a faulting read cannot strand a waiter that has already left the
  condvar queue, and the first fault is reported only after every waiter has
  landed somewhere.
- **`try_acquire_ownerless`** — split into `ownerless_claim_value` (pure
  decision), `acquire_ownerless_with` (the generic CAS retry loop) and the
  user-address wrapper. Necessary because `test_owner_died_relock` drives it
  with a *kernel* `AtomicU32`, which the new accessors correctly reject; the
  self-test now drives the real retry loop through closures instead of testing
  a parallel reimplementation.

Boot test green, all futex self-tests pass (including requeue-PI, PI
owner-death handoff and OWNER_DIED relock).

---

### B-LIMINE-RSDP-REQUEST-CARRIED-THE-WRONG-FEATURE-ID — 2026-08-13 — ✅ FIXED

**What.** `kernel/src/limine.rs` declared the ACPI RSDP request with feature ID
`[0x71ba_7686_3cc5_5f63, 0xb264_4a48_c516_a487]`. That is
`LIMINE_EXECUTABLE_ADDRESS_REQUEST` (`limine/limine.h:648`), not
`LIMINE_RSDP_REQUEST` (`limine.h:555`, `[0xc5e7_7b6b_397e_7b43,
0x2763_7845_accd_cf3c]`).

**Why it hid for so long.** It did not fail — it *succeeded at the wrong
thing*. Limine happily answered the request it was actually asked, so
`RSDP_REQUEST.response()` returned non-null and `RsdpResponse { revision,
address }` overlaid `limine_executable_address_response { revision,
physical_base, virtual_base }`. `address` therefore read back as the kernel's
physical load address. `acpi::init` checked the `"RSD PTR "` signature, found
none, printed one line, and fell back to brute-force scanning ACPI-reclaimable
memory — which finds the real RSDP under QEMU/SeaBIOS. Every boot log carried
the evidence:

```
[boot] RSDP address from Limine: 0x74c43000
[acpi] RSDP not found at provided address 0x74c43000 (tried virt=0xffff800074c43000)
[acpi] Limine RSDP address invalid — scanning memory...
[acpi] RSDP found at phys=0x7f77e000
```

…and a comment in `acpi::init` had even rationalised it as a bootloader quirk
("observed on QEMU+edk2 where Limine returns the kernel load address instead").
It was our bug, not Limine's.

**Impact — not hypothetical; measured.** The scan is a heuristic the RSDP
request exists to avoid, and it was picking the *wrong table*. QEMU publishes
two RSDPs 20 bytes apart; scanning on 16-byte boundaries hits the ACPI 1.0 one
first and stops. So every boot took the legacy path:

| | RSDP | revision | root table |
|---|---|---|---|
| before (scan) | `0x7f77e000` | 0 — ACPI 1.0 | **RSDT** `0x7f77d000` (32-bit pointers) |
| after (bootloader) | `0x7f77e014` | 2 — ACPI 2.0+ | **XSDT** `0x7f77d0e8` (64-bit pointers) |

Both enumerate the same 6 tables on this machine, so nothing was visibly
broken — but the RSDT physically cannot address a table above 4 GiB, and
`init()`'s "prefer XSDT" branch had never once been taken. On UEFI the RSDP
also need not be in either scanned region at all.

**Fix.** Corrected the RSDP ID and added a properly-typed
`ExecutableAddressResponse` request under the ID that was being misused —
which `alternatives::apply()` now needs anyway, to find `.text`'s physical
pages.

**Lesson.** A magic constant that names the *wrong* feature is invisible to
every test that only asks "did we get an answer?". The two-line diff that
would have caught it is checking the ID against `limine/limine.h`, which is
vendored in this repo.

---

### B-AC-INHERITED-AT-KERNEL-ENTRY. An IDT gate does not clear `EFLAGS.AC`, so ring 3 can pre-disable SMAP for every interrupt handler — 2026-08-12 — ✅ FIXED 2026-08-13

**Fix (2026-08-13).** Option 1 below — a real alternatives-patching framework
(`kernel/src/alternatives.rs`, design-decisions §123) — was built and used. Each
of the three ISR stub macros now opens with a 3-byte NOP patch site that
`alternatives::apply()` rewrites to `clac` at boot iff CPUID reports SMAP; the
SYSCALL path was already covered by the widened `IA32_FMASK` (§122). Boot log:

```
[alt] 47 site(s): 47 patched, 0 left at default, 0 error(s)
[alt] Running alternatives self-test...
[alt]   47 patch site(s) in .altinstructions
[alt] Alternatives self-test PASSED
[idt] Running alignment-check-flag (SMAP override) self-test...
EXCEPTION: Breakpoint (#BP) at 0xffffffff8111e73a
[idt]   AC is clear on exception entry: OK (SMAP override closed)
[idt] Alignment-check-flag self-test PASSED
```

That last block is the empirical proof, not an inference from the SDM: the test
sets AC in ring 0, executes `int3`, and the #BP handler observes AC already
clear on entry.

**Still not enabling SMAP.** `smap_enable_blocker()` now reports the *other*
prerequisite — `USER_ACCESSES_ANNOTATED` — and that one turns out to need a
refactor rather than an audit; see
`D-SYSCALL-HANDLERS-HAND-RAW-USER-SLICES-TO-KERNEL-CODE` in the tech-debt
section. The historical analysis below is kept because the reasoning about
*why* an unconditional `clac` was not an option still explains the design.

---

**Original entry (2026-08-12):**

**What.** `EFLAGS.AC` is the SMAP override: while AC = 1, supervisor-mode
accesses to user pages are permitted and SMAP checks nothing. An interrupt gate
clears only TF, NT, RF and VM (Intel SDM Vol. 3A §6.12.1) — **AC is inherited
verbatim from the interrupted context**, exactly as DF was in
`B-NO-CLD-ON-INTERRUPT-ENTRY` below.

AC is *not* privileged: ring 3 sets it with an ordinary `popfq`. So a user
process can set AC and then simply wait for the next timer tick, and every
kernel interrupt/exception handler from then on runs with SMAP disabled.

**Status: latent, not currently exploitable.** `smep_smap::init()` deliberately
does not set CR4.SMAP, so AC is inert today. The bug is that it becomes a real
hole the moment somebody enables SMAP — and it would fail **open and silently**:
nothing crashes, no test goes red, `smap` in kshell reports `ACTIVE`, and the
protection simply does not work. That is the same failure mode as
design-decisions §118 ("a defence that looks sufficient is not").

**Confirmed empirically, not just from the SDM.** `idt::ac_on_entry_self_test()`
sets AC, executes `int3`, and reads AC back as the handler sees it. Boot log:

```
[idt] Running alignment-check-flag (SMAP override) self-test...
[idt]   AC is inherited on exception entry, as expected while
        B-AC-INHERITED-AT-KERNEL-ENTRY is open (SMAP stays disabled): OK
```

**What is already fixed.** The **SYSCALL** half is closed:
`syscall::entry::init()` now masks AC (bit 18) in `IA32_FMASK`, along with NT,
IOPL, RF, ID and the arithmetic flags — the same set Linux masks in
`MSR_SYSCALL_MASK`. Verified in the boot log as `FMASK=0x257fd5`. Only the
IDT-gate path remains.

**Why it is not simply fixed the same way as `cld`.** `clac` raises #UD when
CPUID.SMAP is absent, so an unconditional `clac` in the ISR stubs would break
every pre-Haswell / pre-Zen CPU. Linux emits `ASM_CLAC` at every entry point and
alternatives-patches it to a 3-byte NOP on CPUs without SMAP
(`arch/x86/include/asm/smap.h`); **we have no alternatives-patching framework**,
which is the actual blocker. Options, in preference order:

1. **Build a minimal alternatives framework** (a section of patch sites, rewritten
   once at boot after CPUID). This is the right long-term answer — SMAP will not
   be the last feature that wants it — and it is the only option with zero
   steady-state cost.
2. **Patch the stubs at SMAP-enable time.** A special case of (1) with much less
   machinery: since we control the single place CR4.SMAP is set, overwrite a
   reserved 3-byte NOP in each stub with `clac` there. Cheap, but bespoke.
3. **`pushfq` / `and` / `popfq` in the stub prologue.** Works on every CPU with
   no patching, but `popfq` costs ~20+ cycles on *every* interrupt versus
   `clac`'s ~2, on the hottest path in the kernel. Rejected unless (1)/(2) prove
   impractical.

**Guard against silently mis-enabling SMAP.** `smep_smap::ENTRY_PATHS_CLEAR_AC`
(currently `false`) gates CR4.SMAP via `smap_enable_blocker()`, and
`idt::ac_on_entry_self_test()` asserts that the constant agrees with what an IDT
gate actually does. So flipping the constant without adding `clac` fails the boot
self-test loudly, and adding `clac` without flipping the constant also fails —
the mitigation cannot sit unused, and SMAP cannot be enabled against an entry
path that disables it.

**Where.** `kernel/src/smep_smap.rs` (`ENTRY_PATHS_CLEAR_AC`,
`USER_ACCESSES_ANNOTATED`, `smap_enable_blocker`, `init`, `init_ap`);
`kernel/src/idt.rs` (`ac_on_entry_self_test`, `BP_ENTRY_AC`, the three stub
macros that need the `clac`); `kernel/src/syscall/entry.rs` (FMASK — done).

**Related.** Second instance of the "ring-3 RFLAGS inherited at an IDT gate" bug
class, after `B-NO-CLD-ON-INTERRUPT-ENTRY`. No longer blocked on test coverage:
`B-QEMU-DEFAULT-CPU-HAS-NO-SMEP-SMAP-UMIP` is fixed, so the boot CPU now
advertises SMAP and a `clac` in the stubs would be live code. The remaining
blocker is the alternatives-patching framework (option 1 above).

---

### B-QEMU-DEFAULT-CPU-HAS-NO-SMEP-SMAP-UMIP. The boot test never exercises the supervisor-mode protections — 2026-08-12 — ✅ FIXED 2026-08-13 (`scripts/boot-test.sh`)

**What.** The boot log shows all three protections unavailable, so the code paths
that set CR4.SMEP/SMAP/UMIP never execute under test:

```
[smep_smap] SMEP not supported by CPU
[smep_smap] SMAP not supported by CPU
[smep_smap] UMIP not supported by CPU
[smep_smap]   CR4=0x620
```

QEMU's default CPU model (`qemu64`) does not advertise these features. So
`smep_smap`'s enable paths, and the `stac()`/`clac()` bodies (skipped in the
self-test because the instructions would #UD), are **entirely untested** —
including on the one machine that runs our whole test suite. **SMEP in particular
is assumed to be protecting us and is in fact inactive**, on hardware and in CI
alike, whenever CPUID does not advertise it.

**Fixed by** adding `-cpu "$QEMU_CPU"` to the QEMU invocation in
`scripts/boot-test.sh`, defaulting to `qemu64,+smep,+smap,+umip` and overridable
via the `QEMU_CPU` environment variable. The kernel boots to `BOOT_OK` (262 s)
with SMEP and UMIP genuinely enforced for the first time:

```
[smep_smap] Enabling SMEP (kernel exec of user pages blocked)
[smep_smap] SMAP supported (enablement deferred — IDT entry stubs do not clear
            EFLAGS.AC (B-AC-INHERITED-AT-KERNEL-ENTRY))
[smep_smap] Enabling UMIP (user SGDT/SIDT/SLDT/SMSW/STR blocked)
[smep_smap] CR4 updated: 0x20 → 0x100820
...
[smep_smap]   Active: SMEP=true, SMAP=false, UMIP=true    CR4=0x100e20
[smep_smap]   SMEP enforcement: VERIFIED (CR4 bit set)
[smep_smap]   UMIP enforcement: VERIFIED (CR4 bit set)
[smep_smap]   STAC/CLAC pair: OK (no fault)
```

Two results worth noting: the kernel boots cleanly with SMEP active, i.e. no
kernel code path executes from a user page; and `stac()`/`clac()` executed for
the first time ever (previously skipped as they would #UD), so that
infrastructure is now covered rather than merely written.

**Where.** `scripts/boot-test.sh` (`QEMU_CPU`); `kernel/src/smep_smap.rs`
(the formerly-untested paths).

---

### B-NO-CLD-ON-INTERRUPT-ENTRY. Ring 3 could set the direction flag and make every `rep`-string op in the kernel — `memset`/`memcpy` included — run backwards — 2026-08-12 — ✅ FIXED 2026-08-12 (`kernel/src/idt.rs`)

**What.** None of the three `global_asm!` ISR stub macros in `kernel/src/idt.rs`
(`isr_stub_no_error`, `isr_stub_with_error`, `irq_stub`) issued `cld`. There was
no `cld` anywhere in `kernel/src/` at all.

An IDT gate does **not** clear DF. Loading the new RFLAGS clears TF, NT, RF and
VM; DF is explicitly left alone (Intel SDM Vol. 3A §6.12.1). So DF on entry to
every exception and every hardware IRQ is whatever the interrupted context left
in it — and for a ring-3 interrupt, that is whatever userspace chose. `std` is
an unprivileged instruction, one byte, and ordinary glibc string routines emit
it.

**Why it matters.** The SysV AMD64 ABI requires DF = 0 at every function
boundary, and all compiled kernel code silently depends on it. LLVM lowers
`write_bytes` and `copy_nonoverlapping` — and therefore `[T]::fill`,
`slice::copy_from_slice`, `Vec::extend_from_slice`, and every large struct
move — into `memset`/`memcpy` calls whose `rep stosb`/`rep movsb` bodies walk
**backwards** when DF = 1, writing `[p - len, p)` instead of `[p, p + len)`.
`mm::rawmem::fill_u8` uses `rep stosb` directly for the same reason.

So a ring-3 thread that executes `std` and then waits for a timer tick gets the
entire scheduler, the heap allocator and the serial printer to run with every
string operation reversed. Each one scribbles over the memory immediately
*before* its intended destination. That is:

- **a security hole** — a controlled, unprivileged, no-syscall-needed way for
  userspace to corrupt kernel memory adjacent to whatever the preempting code
  path happens to touch. Linux closes it with a `cld` in its interrupt entry
  path (`arch/x86/entry/entry_64.S`) for exactly this reason;
- **a plausible root cause for B-KNULLJUMP**, the rare nondeterministic heap
  corruption. The shape matches unusually well: it needs userspace running (it
  does not reproduce in early boot), the damage lands at an address depending on
  which instruction the interrupt preempted, the corruption is silent and its
  detection is arbitrarily delayed, and the per-boot probability tracks "did any
  thread happen to be inside a DF = 1 window at a tick boundary" — which is the
  right shape for a base rate near 1-in-120 boots.

**The precondition is confirmed, not assumed.** Disassembling the *exact*
`libc.so.6` that `scripts/create-ext4-rootfs.sh` stages into `rootfs.ext4` finds
one `std` in the whole library, and it is in `__memmove_erms`:

```
  ba8e0:  endbr64                     <-- __memmove_erms
  ba8e4:  mov    %rdi,%rax
  ba8ef:  cmp    %rsi,%rdi
  ba8f2:  jb     ba8ff                ; dst < src  -> forward
  ba8f6:  lea    (%rsi,%rcx,1),%rdx
  ba8fd:  jb     ba902                ; dst < src+n -> overlapping, backward
  ba8ff:  rep movsb                   ; forward path
  ba901:  ret
  ba902:  lea    -0x1(%rdi,%rcx,1),%rdi
  ba907:  lea    -0x1(%rsi,%rcx,1),%rsi
  ba90c:  std                         <-- DF = 1 from here …
  ba90d:  rep movsb
  ba90f:  cld                         <-- … to here
  ba910:  ret
```

So any ring-3 `memmove(dst, src, n)` with `src < dst < src + n` — an ordinary
forward-shifting overlapped copy, which is what stdio buffer compaction and
insert-at-front do — runs with **DF = 1 across the whole `rep movsb`**. That
window is not an instruction or two: `rep movsb` is architecturally interruptible
between iterations (RIP stays on the prefix so it can resume), so the window is
*proportional to the copy length*. A large overlapping memmove is a wide open
door for a timer tick.

**And the corruption *class* matches, not just the timing.** B-KNULLJUMP is
specifically a jump through a **null** code pointer (`RIP=0x0`, `error=0x10` —
kernel instruction fetch of a not-present page). That is exactly what a
backwards `memset` manufactures. Most `memset`s in Rust code are *zero* fills —
`vec![0; n]`, `MaybeUninit::zeroed`, `Default`-style struct initialisation, a
cleared buffer — and a zero fill running backwards writes zeros over
`[dst - n, dst)`, i.e. it zeroes whatever object happens to sit immediately
*before* the buffer being cleared. Any function pointer in that region becomes
null, and the next call through it lands at `0x0`.

So the hypothesised chain is fully concrete:

1. ring 3 calls `memmove` with an overlapping forward shift → `std`,
2. a timer tick lands inside the length-proportional `rep movsb` window,
3. the kernel is entered with DF = 1 (no `cld` at the gate),
4. any zeroing `memset` on that path clears the memory *before* its buffer,
5. a callback/vtable/return-address slot in that memory becomes null,
6. the kernel later calls through it → `RIP=0x0`.

Step 6 is B-KNULLJUMP's exact observed signature, and steps 1–3 are now
established fact rather than conjecture.

**Still unproven: that this is what actually happened.** What is established is
that (a) the kernel entered with whatever DF ring 3 had, (b) ring-3 glibc
demonstrably sets DF for interruptible, length-proportional windows, and (c) the
resulting corruption primitive produces precisely the observed failure class.
What is *not* established is that the one caught instance came through this path
rather than another — a use-after-free on a callback pointer, the original
suspicion recorded in `B-KNULLJUMP-SIGNAL`, produces the same signature and
remains possible. The bug above is worth fixing on its own terms regardless. If
B-KNULLJUMP survives this fix, the hypothesis is disproved.

**Note the asymmetry that hid this.** The SYSCALL path was already correct:
`kernel/src/syscall/entry.rs` programs `IA32_FMASK` bit 10, so the CPU clears DF
as part of the transition, and the comment there even says *"Bit 10 = DF
(direction) — ensure forward string ops in kernel."* Whoever wrote that knew the
hazard; only the IDT-gated half was left uncovered. A grep for `DF` or
`direction` finds the handled case and nothing to suggest the unhandled one,
which is why review kept passing over it.

**The fix.** `"cld"` as the *first* instruction of all three stub macros —
first, rather than tucked in just before the `call`, so no later edit can
insert a string operation ahead of it. No `std` is needed on the way out:
`iretq` restores the whole saved RFLAGS, DF included, so the interrupted context
gets its own flag back untouched.

`mm::rawmem::fill_u8` also grew its own `cld` inside the `asm!` block (and
consequently dropped `options(preserves_flags)`), so the helper is correct on
its own terms rather than by trusting a caller-side invariant. Its SAFETY
comment previously asserted that "the SysV ABI guarantees DF = 0 at every
function boundary" — true of compiled code, but not of the machine, and exactly
the assumption this bug violates. That comment has been corrected.

**Regression test.** `idt::df_on_entry_self_test()`, run at boot right after
`mm::rawmem::self_test()`. It sets DF and executes `int3` **in a single `asm!`
block** — they must not be separable, or the compiler could schedule a `memcpy`
into the window and corrupt memory with the very bug under test — and checks a
flag recorded at the top of `handle_breakpoint`, which observes DF as the
handler sees it. A second block confirms `iretq` hands the caller's DF back.
Without the stub `cld`, the first assertion is the only visible failure; the
real symptom is silent corruption somewhere else entirely, which is why a direct
test earns its keep here.

### TD-HARNESS-RUN-TIMEOUT-COULD-NOT-LAUNCH-A-SHELL-SCRIPT-AND-BARE-BASH-MEANT-WSL. The documented boot-test invocation never ran the boot test — 2026-08-12 — ✅ FIXED 2026-08-12 (`scripts/proctree.py`)

**What.** `CLAUDE.md` documents the canonical hang-proof invocation as:

```bash
python scripts/run-timeout.py 60 ./scripts/boot-test.sh
```

That command could never work on this machine, and both of its failure modes
produced a *misleading* run rather than an error:

1. **A `.sh` cannot be launched by `CreateProcess`.** Windows has no shebang
   handling, so `Popen(["./scripts/boot-test.sh"])` fails with `[WinError 193]
   %1 is not a valid Win32 application`. `run-timeout.py` correctly returned
   125 — but a caller that appends `; echo EXIT=$?`, or otherwise reports the
   *wrapper's* status rather than the runner's, converts "the boot test never
   started" into a green result. That is exactly what happened: a boot test
   reported exit 0 having never booted anything.

2. **Naming `bash` explicitly is worse, because it appears to work.** From a
   native-Windows parent, `CreateProcess` searches `System32` *before* `PATH`,
   and `C:\Windows\System32\bash.exe` is the **WSL launcher**. So
   `Popen(["bash", "scripts/boot-test.sh"])` runs a *Linux* bash in a different
   filesystem namespace, where `/c/Program Files/qemu`, `cygpath` and
   `taskkill //F` do not exist. The observed symptom was
   `ERROR: qemu-system-x86_64 not found` — a failure with nothing whatsoever to
   do with the code under test, and one that invites you to go looking for a
   broken QEMU install.

   The trap is sharpened by `shutil.which("bash")` **not** reproducing it: it
   walks `PATH` and answers Git Bash. So the PATH lookup and the actual launch
   disagree, and only the launch is authoritative.

**Why it matters beyond the boot test.** `proctree.Tree` is the single launch
point behind both `run-timeout.py` and `run_captured`, and CLAUDE.md directs
*all* potentially-hanging commands through it. Any script-based harness
(`boot-test.sh`, `flake-hunt.sh`, `wedge-soak.sh`, the `p3x-check.sh` gates)
was unreachable through the one runner that guarantees process-tree cleanup —
which pushes callers back onto bare `timeout`, the orphan-leaking tool
`proctree` exists to replace.

**The fix.** `proctree.resolve_command()`, called from `Tree.__init__` so every
caller gets it:

- argv[0] a shell script (by `.sh`/`.bash` extension, or by parsing its `#!`
  line) → interpose an absolute bash. The shebang interpreter is matched on its
  **basename** after resolving one level of `/usr/bin/env`, so
  `#!/home/shared/bin/python3` is not mistaken for a shell (a substring test
  for `sh` would have been).
- argv[0] a bare `bash`/`sh` → rewritten to an absolute MSYS/Git-Bash path,
  with `System32`/`SysWOW64`/`Sysnative` rejected by **location** so the WSL
  shim can never be selected.
- No shell found → raise `OSError` with a specific message. Falling back
  silently is the failure this exists to prevent, so it must be loud.
- Everything else (`cargo`, `taskkill`, a real `.exe`, an explicitly-pathed
  interpreter, a non-list command) passes through untouched, and on POSIX the
  whole function is a no-op — the kernel honours `#!` itself.

`SLATE_BASH` overrides the search for a host whose shell lives elsewhere.

**Tests.** `scripts/test-proctree.py` (plain `python`, no pytest dependency):
29 checks covering passthrough, script/bare-shell rewriting, WSL-shim
detection, shebang parsing (including the `sh`-inside-a-path and `fish`
false-positive cases), and two end-to-end runs — one asserting the resolved
shell is really MSYS, one asserting `Tree(["...sh"])` runs a script *and
reports the script's own exit code* rather than a launch failure.

---

### BUG-LIVENESS-DEADLINE-FALSE-FIRE. The boot-window liveness watchdog reported a hang on *every* healthy boot — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** Every green boot-test run — one that reaches `BOOT_OK` and exits 0
— printed alarming watchdog reports and buried the serial log under ~14,000
lines of task-table dump:

```
[liveness] BOOT DEADLINE EXCEEDED: still armed 200s after arming (no BOOT_OK).
           The progress-based detectors did not trip, so this is a livelock or
           partial hang … Dumping task table:
…13,800 lines…
[liveness] SYSTEM HANG: no task-level forward progress for 15+ seconds
           (useful_work=543, all CPUs idle-ticking). Dumping task table:
```

Reproduce: any `./scripts/boot-test.sh` run; the reports are in
`build/serial-test.txt` (also present in the archived
`build/serial-loop-{1,2,3}.txt`). A watchdog that cries wolf on every run is
worse than no watchdog — a *real* dump was indistinguishable from the noise.

**Three independent false-fire modes, all in `kernel/src/sched/mod.rs`.**

1. **The wall-clock boot deadline was a hardcoded constant that went stale.**
   `LIVENESS_BOOT_DEADLINE_NS` was 200 s, tuned on 2026-07-02 against a
   then-measured healthy armed window of **67.7 s**. The Path-Z ring-3 toolchain
   battery has grown a lot since; by 2026-07-27 a healthy boot needed ~350 s to
   `BOOT_OK` and the armed window alone exceeded 200 s, so the deadline fired on
   every run. `scripts/boot-test.sh`'s own `TIMEOUT` comment already documents
   that "the suite keeps growing" — the constant could not help drifting.

2. **The total-hang detector's progress signal does not cover kernel-side boot
   work.** `USEFUL_WORK_TICKS` only advances for a timer tick that preempted
   ring-3 code (`from_user`) or a CPU with a *queued* task
   (`local_has_real_work`). Neither holds during the long kernel-side stretches
   of boot: `kmain` runs with no queued task, and a *starting* ring-3 process
   spends nearly all its wall time inside the kernel on its own behalf (ELF
   load, demand-paging storm, filesystem I/O), so ticks land in kernel mode with
   an empty run queue. Measured in `build/serial-loop-1.txt`: `useful_work=8`
   after `heartbeat=2501` (25 s of ticks) — while the log shows the kernel busily
   spawning ring-3 fastpy processes. Healthy boot, "total hang" verdict.

3. **The deadline dump caused the second report.** Emitting ~13,800 lines over a
   115200-baud UART from inside the timer ISR stops all task progress for
   minutes, which then satisfied detector 2 — a watchdog whose own diagnostic
   trips another watchdog.

A fourth defect hid all of the above: `liveness_disarm()` logs the measured
healthy armed duration (the number needed to keep the deadline honest) **only
when still armed**. Once a detector had disarmed us the line vanished — so the
one measurement that would have contradicted the stale constant was never
printed on any run where it mattered.

**Fix (proper, not a re-tune).**

- **Derive the deadline from the harness's own timeout instead of hardcoding
  it.** `scripts/boot-test.sh` now always passes
  `sched.boot_deadline_ms=$((TIMEOUT * 1000))` on the Limine cmdline, and
  `liveness_arm()` computes
  `deadline = budget − LIVENESS_DEADLINE_MARGIN_NS (45 s) − (monotonic now)`.
  Subtracting the arm timestamp converts the harness's QEMU-relative budget into
  the armed-relative units the backstop measures in, so the fire time lands 45 s
  of wall-clock before the kill regardless of how much of the budget early boot
  ate. The invariant becomes structural: the watchdog fires iff the harness was
  about to give up anyway, and raising `--timeout` moves both in lockstep.
  `LIVENESS_BOOT_DEADLINE_DEFAULT_NS` (900 s) covers boots with no such cmdline
  (real hardware, hand-rolled QEMU), where nothing is going to kill us.
- **Add a serial-output progress signal and gate both detectors on silence.**
  `serial::_print` bumps `OUTPUT_COUNT` (read via `serial::output_count()`); the
  scheduler snapshots it each interval and both the total-hang and busy-livelock
  detectors stand down whenever anything was printed during the interval. This
  kernel narrates its boot continuously, so a *silent* interval means execution
  really stopped and a chatty one means it did not — the same criterion
  `boot-test.sh --stall-secs` uses from the outside. It also removes mode 3 for
  free: the dump itself keeps the counter moving.
- **Always log the armed duration at disarm**, and shout when a detector had
  already disarmed us on a boot that went on to reach `BOOT_OK` — that
  combination *is* a false positive and should be impossible to miss.
- `liveness_arm()` now also logs the deadline it chose and whether it came from
  the harness or the fallback.

**Tradeoff accepted.** A livelock that keeps printing now escapes the two 15 s
detectors. It does *not* escape the wall-clock boot deadline, which catches
every hang mode by construction — and that deadline is now trustworthy, which it
was not before. Recorded in `design-decisions.md`.

**Verification.** New boot self-test coverage in `test_liveness_watchdog`:
`parse_boot_deadline_ns` (key found anywhere in the cmdline; absent/malformed
rejected), `armed_relative_deadline_ns` (480 s budget − 60 s spent − 45 s margin
= 375 s; a spent budget yields `None` rather than wrapping), and a silence-gate
sequence that freezes the useful-work counter while printing each interval and
asserts the watchdog stays armed with a zero stall counter. Plus a full boot
test whose log must contain no `BOOT DEADLINE EXCEEDED` / `SYSTEM HANG` line and
must contain the `[liveness] disarmed after …` measurement without the
FALSE POSITIVE warning.

### BUG-EXEC-ARGV-CAPACITY-OVERFLOW. `parse_packed_strings` pre-allocated `Vec::with_capacity(usize::MAX)` for exec argv/envp → kernel panic ("capacity overflow") on any `execve`/`SYS_EXECVE`/spawn with a non-empty argv — 2026-07-23 — ✅ RESOLVED 2026-07-23

**Symptom.** Any userspace-initiated exec that supplied a non-empty argv (or envp) panicked the kernel with `capacity overflow` the moment the exec handler unpacked its arguments. Latent for a long time because virtually every exercised exec path passed `argv_ptr == 0` (empty argv); it only surfaced when the new fastpy `os.execv` runner (`fastpy-run`) handed `os.execv(path, [cmd, target])` — a two-element argv — down through posix `execv()` → `SYS_PROCESS_EXEC`.

**Root cause.** `parse_packed_strings(data, max_count)` (`kernel/src/syscall/handlers.rs`) did `alloc::vec::Vec::with_capacity(max_count)`. `max_count` is only an *upper bound*; the exec handler (`sys_process_exec_with_frame`, handlers.rs ~5273/5278) passes `usize::MAX` to mean "no limit, derive the count from the packed data." `Vec::<&[u8]>::with_capacity(usize::MAX)` overflows the internal `capacity × size_of::<&[u8]>()` (×16) layout computation, which panics with `capacity overflow` — an unconditional kernel DoS reachable from ring 3 by anyone able to exec with arguments.

**Fix.** Clamp the pre-allocation to a tight, real upper bound: a packed buffer of length `data.len()` can hold at most `data.len()` NUL-separated strings plus one unterminated tail, so `cap = data.len().saturating_add(1).min(max_count)`. This hardens *all* callers (the argc/envc-bounded spawn callers and the `usize::MAX` exec callers alike). Verified end-to-end by the `self_test_fastpy_slateos_run` ring-3 self-test (fastpy-run resolves `cat` over PATH, `os.execv`s it, `cat` exits with the 23-byte count) booting green with no panic.

### BUG-EXT4-SPARSE-READ. ext4 extent reader collapsed sparse holes — every sparse file read back corrupted (data shifted left into the holes) — 2026-07-23 — ✅ RESOLVED 2026-07-23

**Where:** `kernel/src/fs/ext4/driver.rs` — `read_range_from_tree` (the page-cache
fill primitive behind `read_at`/`mmap`), `read_extent_data` + `read_extent_tree_recursive`
(the full-file `read_file_data` path).

**What:** All three extent readers assembled file data by **appending** each
allocated block's bytes to the output (`result.extend_from_slice(...)`, tracking
position by `result.len()`), and **never consulted `extent.ee_block`** (the block's
logical offset). For a *contiguous* file this is fine. For a **sparse** file (one
with unmapped logical blocks — holes), the gaps were simply skipped, so every block
after a hole was placed at the wrong offset — the data after each hole got dragged
left into the hole. The read came back the correct *length* only because the read
path pre-zeroes a full-size page-cache buffer (`read_file_routed` → `read_through`),
and the *zero-count* happened to match — but the bytes were permuted, so any hash
differed.

**How it manifested:** the fastpy self-test ELFs (moved onto the rootfs disk for
TD-KERNEL-EMBED-BLOAT) are **>50 % zeros** and stored sparse by `cp`/mkfs (e.g.
`fastpy-hello.elf`: 3 475 528 bytes, 1 777 808 zero, ~214 hole blocks). Loaded from
`/mnt/tests`, every one validated as an ELF (header/TLS in the first, hole-free
blocks) and reached `Zombie`, then **jumped through a null function pointer**
(`rip=0x0`, exit code -8) because its `.data`/relocated code had been shifted — 100 %
of fastpy ring-3 tests failed identically. Dense files (glibc `.so`) were unaffected,
which is why real-glibc Path-Z tests always passed. Confirmed by checksumming the
loaded ELF in-kernel: FNV `0x42eb…bf12` vs host `0x77f4…bf12` (same len, same zero
count), then byte-exact match after the fix.

**Fix:** rewrote all three readers to place each allocated block at its **absolute
logical offset** in a pre-zeroed output buffer, leaving holes and unwritten extents
as zeros. Extracted the placement arithmetic into a pure `block_copy_placement()`
helper (unit-tested in `#[cfg(test)] mod placement_tests` and by the boot-time
`test_block_copy_placement` in `driver::self_test`). Verified: all 55 sparse fastpy
ring-3 self-tests pass, byte-exact ELF load, green boot.

### BUG-BOOTTEST-BOOTOK-SUBSTRING. `boot-test.sh` reported PASSED on a livelocked boot because its `grep -q "BOOT_OK"` matched the substring inside the livelock diagnostic "…still armed 200s after arming (no BOOT_OK)…" — 2026-07-22 — ✅ RESOLVED 2026-07-22

The success marker is printed as a standalone line (`serial_println!("BOOT_OK")`),
but the detector used an UNanchored `grep -q "$WAIT_MARKER"`.  The liveness
watchdog's `BOOT DEADLINE EXCEEDED … (no BOOT_OK)` message contains the substring
`BOOT_OK`, so on a genuine livelock (the fastpy-nice tool once busy-spun at a
raised priority and starved the harness) the harness still printed
"BOOT_OK detected … Boot test PASSED".  Fixed by anchoring all four match sites
to line start (`grep -q "^$WAIT_MARKER"` / `^BOOT_OK`) in `scripts/boot-test.sh`,
so only the standalone marker line counts.  Found while validating the
fastpy-nice (nice→priority) self-test.

### TD-FRAME-OWNER-1GIB. `frame_owner` ownership array only tracks the first 1 GiB of RAM (fixed `[u8; 65536]`) — 2026-07-22 — ✅ RESOLVED 2026-08-14 (array made dynamic *and* wired into the allocator)

**Where:** `kernel/src/mm/frame_owner.rs` — `const MAX_FRAMES: usize = 65536;`
and the `OwnerArray([u8; MAX_FRAMES])` static; `set`/`get`/`clear` no-op when
`frame_idx >= MAX_FRAMES`.

**What:** Frame index = `phys_addr / FRAME_SIZE` (16 KiB), so 65536 frames covers
only the first **1 GiB** of physical memory. On any machine with more RAM, a
frame allocated above 1 GiB gets owner `Unknown` — its owner tag is silently
dropped. This is the *same* fixed-window bug that affected the per-frame cgroup
array (now fixed — see BUG-CGROUP-1GIB below), but for the frame *owner*
tracking.

**Impact:** ownership tracking is **diagnostic only** (leak reporting, the
`[mm]` owner-census stats, and the owner self-test), so a wrong `Unknown` above
1 GiB degrades diagnostics but does **not** corrupt allocation or accounting.
That is why this is low severity and was left open while the cgroup array (which
*is* correctness-affecting) was fixed immediately.

**Even lower priority than it looks:** `frame_owner::set`/`clear` currently have
**no callers** in the allocator (verified 2026-07-22 — the `Owner` enum is only
passed as a label to the separate `alloc_trace` ring buffer). So the `OWNERS`
array is never populated in production; it is exercised only by the module's own
self-test. The 1-GiB ceiling therefore has no real effect today. The proper fix
below should be done *together with* wiring `set`/`clear` into the alloc/free
paths if per-frame owner census is ever actually wanted; refactoring the array
to be dynamic in isolation (while it stays unwired) is busywork.

**Proper fix:** make `OwnerArray` dynamic exactly like the cgroup array now is —
carve `total_frames` bytes from the frame-allocator metadata region in
`frame::init` (or a dedicated init hook), publish a base-pointer + length pair,
and bounds-check `set`/`get`/`clear` against the dynamic length. The metadata
region already reserves per-frame bytes; adding one more `total_frames`-byte
sub-array is the same pattern used for `page_info`/`refcount`/cgroup.

---

**RESOLVED 2026-08-14.** Both halves were done together, exactly as the note
above insisted — resizing the array alone would have been busywork while
`set`/`clear` still had no callers.

**1. Dynamic array.** `const MAX_FRAMES = 65536` and the
`OwnerArray([u8; MAX_FRAMES])` static are gone. `frame::plan_metadata` now
reserves a fourth per-frame sub-array (`owner_offset = cgroup_offset +
total_frames`), `frame::init` zeroes it (`0` == `Owner::Free`) and publishes it
via `frame_owner::init_storage(ptr, total_frames)`; `OWNERS_PTR`/`OWNERS_LEN`
back a `slot()` helper that every accessor goes through. Same pattern as the
cgroup array (BUG-CGROUP-1GIB). Boot confirms the carve:
`[mm] Metadata: ... [page_info: 327680B, refcount: 655360B, cgroup: 327680B, owner: 327680B]`.

**2. Actually wired up.** `tag_alloc_owner`/`untag_free_owner` are called from
all six allocator choke points: both per-CPU fast paths in `alloc_frame`, the
zero-pool pop in `alloc_frame_zeroed`, `alloc_order`, `alloc_order_constrained`,
`free_frame` and `free_order`. The zero-pool refiller tags parked frames
`Owner::ZeroPool`; the consumer re-tags on pop.

**3. Ambient owner context.** The allocator cannot know its caller, so
attribution comes from `OwnerScope` — a cache-line-padded per-CPU RAII guard
that saves the previous tag and restores it on drop, so it nests correctly even
when an IRQ handler allocates inside another subsystem's scope. Tagged so far:
page tables (`page_table.rs` PT-page pool refill), kernel stacks (`kstack.rs`),
slab + large heap (`heap.rs`), CoW (`cow.rs`), and user anon pages (`vma.rs`
demand paging, `idt.rs` stack growth). Untagged allocations record
`Owner::Unknown`, which is honest rather than wrong.

Known accuracy limit, documented on `OwnerScope`: the tag is per-CPU, so a task
preempted and migrated mid-scope restores onto the new CPU and can mis-attribute
a handful of frames. Accepted deliberately — this is diagnostic-only, and a lock
or a per-task field reachable from boot/IRQ contexts would cost more on the
allocation hot path than the precision is worth.

**Verified.** Boot PASSED 273s; the rewritten self-test reports:

```
[frame_owner]   Covers all 327680 frames (5120 MiB): OK
[frame_owner]   High frame 327679 (> old 65536-frame window): OK
[frame_owner]   Alloc/free tagging round-trip: OK
[frame_owner]   OwnerScope nesting: OK
[frame_owner]   summary/find_by_owner: OK
[frame_owner]   Stats: sets=155837, clears=299809
```

The second line is the direct regression test for this bug. The nonzero
`sets`/`clears` are the proof that the allocator now reaches this module at all.

*On `clears` > `sets`:* these count **calls, not transitions**. `clear()` runs on
every free, including frames that were never tagged — anything allocated before
`init_storage` published the array, plus rollback paths that free via
`free_order_inner` without a matching tagged alloc. Not a leak; the per-frame
state is still a correct free/allocated mirror, as test 4's round-trip shows.

**The self-test had to be rewritten, not just extended.** The old one wrote raw
indices (100, 200, 300…) straight into the array. That was harmless while
nothing populated it, but the moment the allocator went live those writes would
corrupt *real* frames' records. It now allocates and frees actual frames for
every check, and saves/restores around the one raw-index probe.

**Side effect: found and fixed a latent bug.** Making `current_owner()` run on
every allocation pulled `smp::fast_cpu_index()` into early boot and exposed
B-SMP-FAST-CPU-INDEX-PANICS-BEFORE-APIC-INIT (tier-3 APIC fallback reads a null
APIC base before `apic::init` — panic in debug, wild read in release). See that
entry.

### BUG-CGROUP-1GIB. (RESOLVED 2026-07-22) per-frame cgroup array only covered the first 1 GiB → cgroup accounting leak above 1 GiB

**Where:** `kernel/src/mm/frame.rs` — was `const CGROUP_MAX_FRAMES = 65536;` +
`FRAME_CGROUP([u8; 65536])` static.

**What (was):** the per-frame cgroup-id array (used by `set_frame_cgroup`/
`get_frame_cgroup` to remember which cgroup a frame was charged to, so
`uncharge_cgroup_free` can uncharge the right group on free) was a fixed 65536
entries = 1 GiB window. A frame allocated above 1 GiB was charged
(`mem_charge` succeeded) but its per-frame record was dropped, so on free
`get_frame_cgroup` returned 0 and the group was **never uncharged** — a
monotonic cgroup memory-usage leak that eventually made a limited cgroup deny
allocations it should have allowed. Latent because every boot-test ran at
`-m 512M` (all frames < 1 GiB); surfaced when boot-test RAM was raised to 3 GiB
and the mm cgroup round-trip self-test panicked (`frame.rs:3035`, "charge must
record the cgroup id", `get_frame_cgroup` returned 0 for a high frame).

**Fix:** the cgroup array is now **dynamically sized to `total_frames`** and
carved from the frame-allocator metadata region in `frame::init` (right after
`page_info` + `refcount`); `FRAME_CGROUP_PTR`/`FRAME_CGROUP_LEN` publish it and
the accessors bounds-check against the live length. Covers all installed RAM.
Verified: boot GREEN at `-m 3072M`, `[mm] Cgroup per-frame tracking: OK` +
`Cgroup charge/uncharge round-trip (no double-charge): OK`.

### BUG-OPENAT2-RESOLVE-NO-SYMLINKS-IGNORED. `openat2`'s `RESOLVE_NO_SYMLINKS` was accepted but never enforced — symlinks in the path were still followed — 2026-07-22 — ✅ RESOLVED 2026-07-22

**What:** `sys_openat2` (`kernel/src/syscall/linux.rs`) accepted the
`RESOLVE_NO_SYMLINKS` resolve bit and then forwarded to the normal openat
path with a comment claiming it was "trivially satisfied … no symlinks at
all". That claim is **false** — this kernel fully supports symlinks (memfs
*and* ext4). So `openat2(dirfd, path, {resolve: RESOLVE_NO_SYMLINKS})` would
silently follow symlinks in any path component, defeating the security
feature (RESOLVE_NO_SYMLINKS exists specifically to block symlink-swap
attacks). It is strictly stronger than `O_NOFOLLOW`: it must reject a
symlink in *any* component — parent or final — whereas O_NOFOLLOW guards
only the final one.

**Fix (2026-07-22):** Added `Vfs::resolve_no_symlinks` (kernel/src/fs/vfs.rs)
— a resolver mode that returns `TooManyLinks` (→ `ELOOP`) on encountering a
symlink in any component (threaded via a new `no_symlinks` param on
`resolve_inner`; dcache bypassed since it stores symlink-followed results).
Added `OpenFlags::NO_SYMLINKS` (bit 8) to kernel/src/fs/handle.rs; `open()`
uses the strict resolver when it is set. Threaded a `no_symlinks` bool from
`sys_openat2` → `sys_openat_ex` → `open_common`/`open_kernel_path_install`,
which OR the bit into the native OpenFlags (non-forgeable: a program can
only opt *into* stricter resolution). Handle self-test section 18 proves:
(a) parent symlink followed without the flag, (b) parent-component symlink →
ELOOP with NO_SYMLINKS (the case O_NOFOLLOW misses), (c) final symlink →
ELOOP, (d) symlink-free path still opens. NO_XDEV/NO_MAGICLINKS remain
genuinely trivial (no mid-walk bind mounts, no /proc magic links).

### BUG-MUNMAP-NO-TLB-FLUSH. `sys_munmap`/`sys_mmap` never flushed the TLB → freed frame stayed writable via stale TLB → buddy free-list corruption → kernel #PF — 2026-07-22 — ✅ RESOLVED 2026-07-22

**What:** `sys_munmap` (kernel/src/syscall/handlers.rs) walked the range calling
`page_table::unmap_frame`, which only *clears the page-table entries* and
(per its own doc-comment) leaves TLB invalidation to the caller — but
`sys_munmap` never flushed the TLB. It then immediately returned each frame to
the buddy allocator via `frame::free_frame`. The committed-`sys_mmap` path had
the mirror-image gap: `map_committed_range` also documents "caller must flush",
and the handler didn't.

**Symptom / repro:** Under heavy anonymous mmap churn (the fastpy runtime's
malloc grows/shrinks the heap by repeatedly `mmap`/`munmap`-ing single frames —
visible in the serial log as alternating `[mmap] Committed mapped …` /
`[mmap] Unmapped 1 frames at …` on the *same* VA), the process kept a stale
VA→frame TLB entry after `munmap`. Because the frame was already freed, its
first 16 bytes were reused as the buddy allocator's intrusive `FreeNode`
(next/prev physical-address links). The process's still-cached writes to the
old VA (heap pointers in the `0x60_0000_0000` mmap region) overwrote that node,
so a *user virtual address* leaked into the free list as a bogus "physical"
next-pointer. A later `alloc_frame` walked the list and dereferenced
`phys_to_virt(0x6000070000)` = `0xffff806000070008` (physmap alias of a
~412 GB "frame" that doesn't exist) → **unrecoverable kernel #PF, write,
not-present** in `BuddyAllocator::remove_free` ← `pop_free` ← `alloc_inner`.
Triggered on-target by the fastpy package-manager generations self-test
(`self_test_fastpy_slateos_pkg_gen`), whose commit/rollback file copies do
enough heap churn to cross the threshold; the lighter dependency-lifecycle test
that runs just before it did not.

**Fix:** Added `mmap_flush_range(start, end)` in handlers.rs (mirrors
`mprotect_flush_range`: per-page `crate::tlb::flush_range` for ≤64×4 KiB pages,
`crate::tlb::flush_all` above that) and call it (a) in `sys_munmap` after the
unmap loop, before the frames can be recycled, and (b) in the committed
`sys_mmap` path after `map_committed_range`. This closes the window where a
freed frame remains reachable through a stale translation.

**Why it lay hidden:** most earlier ring-3 tests either did little heap churn or
happened not to reuse a frame while its stale TLB entry was still live; the
generations test was the first to reliably cross the reuse-while-stale window.

**Class audit (2026-07-22):** swept every other unmap-then-free caller for the
same missing-flush gap. All are already correct:
- `linux.rs::sys_munmap` (Linux ABI, ~7104) and `sys_brk` shrink (~7321) go
  through `unmap_user_range`, which flushes per-page (`tlb::flush_range(va,1)`,
  ~10878) as it clears each PTE. `sys_mremap` never unmaps (returns ENOMEM).
- Rollback paths — `drm_mmap_dumb_rollback` (~10665), `linux_file_mmap_rollback`
  (~10922), the MMIO-map rollback (handlers.rs ~894/914), and the shm_map
  rollback (handlers.rs ~2801) — undo state created earlier in the *same*
  syscall. Userspace never runs between the map and the unmap, and file data is
  read into frames via the HHDM alias (not the user VA), so no TLB entry is ever
  cached; device/MMIO and shm frames are ref-decremented, not hard-freed, so
  even a stale entry couldn't corrupt the buddy free list. `linux_file_mmap_fill`
  frees (11314/11335/11343) are for frames that were never mapped.
- Process teardown (pcb.rs `clear_user_address_space`) is covered by the CR3
  reload on the next context switch, which flushes the whole TLB.
The native `handlers.rs::sys_munmap`/`sys_mmap` (fixed above) was the only path
where userspace had run and dirtied TLB entries before freeing to the allocator.

### TD-NATIVE-MPROTECT. No native `SYS_MPROTECT` handler — native `mprotect()` returns ENOTSUP — 2026-07-21 — ✅ RESOLVED 2026-07-22

**What:** The posix libc's native syscall path exposes `SYS_MPROTECT = 22`
(kernel/src/syscall/number.rs), but no handler is registered for it in the
*native* dispatch table (kernel/src/syscall/dispatch.rs). A native
`mprotect()` therefore resolves to NotSupported → `ENOTSUP`. Only the
**Linux-ABI** mprotect (linux.rs, Linux syscall #10 — `sys_mprotect`) is
implemented, and it's fully functional for the Linux compatibility path.

**Proper fix:** Register a native mprotect handler at 22 that reuses the
Linux mprotect core (`sys_mprotect`'s VMA-coverage + per-4KiB PTE walk) but
returns `KernelError` codes rather than Linux errno, so posix's
`errno::translate` maps it uniformly with the rest of the native ABI.
Options: (a) factor the mprotect core into a `fn -> KernelResult` shared by
both the Linux and native handlers (cleanest, preferred); or (b) register the
existing `sys_mprotect` at 22 and switch posix's `mman::mprotect` to a
Linux-style errno translate (posix errno values already equal Linux errno
values). Tracked in todo.txt.

**Impact:** Low for current work — the fastpy/initiative-F binaries and the
crt TLS path do not call mprotect. glibc RELRO / pthread guard pages use the
Linux ABI path, which is unaffected.

**Fix (2026-07-22):** Took option (a) — the cleaner, DRY route. Factored the
mprotect implementation into an ABI-neutral core in `kernel/src/syscall/linux.rs`:
- `mprotect_validate_core(addr,len,prot) -> KernelResult<Option<u64>>` runs the
  Linux `do_mprotect_pkey` gate order (align/len==0/overflow/prot-bits/userspace)
  and now returns `KernelError` (`InvalidArgument`/`OutOfMemory`) instead of a
  Linux-errno `SyscallResult`. A thin `mprotect_validate_args` wrapper still
  produces the `MprotectValidation` form for `sys_pkey_mprotect` (unchanged).
- `pub(crate) fn mprotect_core(addr,len,prot) -> KernelResult<()>` holds all the
  page-table work (VMA-coverage check, `protect_vma_range` bookkeeping, per-4 KiB
  PTE `change_flags_4k`, single batched `mprotect_flush_range` shootdown), now
  propagating `KernelError` via `?` and returning `Ok(())` on success.
- `sys_mprotect` (Linux ABI, syscall #10) is now a 4-line wrapper mapping
  `Err(e) => linux_err(linux_errno_for(e))`.
- New native handler `syscall::handlers::sys_mprotect` calls the same
  `mprotect_core` and returns raw `KernelError` codes; registered at
  `SYS_MPROTECT` = 22 in `syscall::dispatch`. posix `mman::mprotect`'s
  `errno::translate(syscall3(SYS_MPROTECT,…))` now maps 0→success,
  `InvalidArgument`→EINVAL, `OutOfMemory`→ENOMEM, `NoSuchProcess`→ESRCH — no more
  ENOTSUP.

Both ABIs share one implementation of the actual work, so there is no drift.
Verified by a new native dispatch self-test `test_dispatch_mprotect_native`
(proves the handler is registered — not `NotSupported` — and that the gate order
returns native error codes for misalign/len0/bad-prot/overflow) plus the
pre-existing Linux `self_test_mprotect_validation`/`self_test_mprotect_flush_range`
which continue to exercise the shared core.

### B-TCC-LIBTCC1-MAIN. On-target tcc one-shot compile+link spuriously fails with `unresolved reference to 'main'` (exit 1) when the source emits one extra undefined symbol (e.g. the `memset` a struct/aggregate brace-initialiser synthesises) — ON-TARGET-ONLY, **COULD NOT REPRODUCE (22 on-target compiles) — DOWNGRADED TO WATCH**, REGRESSION-GUARDED 2026-07-16

**UPDATE 2026-07-16 (could not reproduce; downgraded WATCH; regression
guard added).** On-target instrumentation was built and run to reproduce
this live: a boot self-test (`self_test_tcc_diag_brace_init`, since
removed) compiled **four distinct `memset`/`memcpy`-emitting constructs**
(constant brace-init, runtime-value brace-init, a 256-byte zero-init
array, and a struct-to-struct copy) **five times each = 20 on-target
`tcc -vv` compiles**, plus two earlier single shots = **22 on-target
compiles that all carried the extra undefined `memset`/`memcpy` symbol.
Every one linked and ran cleanly (exit 0, valid dynamic ELF).** The
documented deterministic trigger — "one extra undefined symbol makes the
on-target link lose `main`" — is therefore **disproven**: `memset`
presence is *not* sufficient to reproduce the failure. The original Part
47 failure was thus either genuinely **intermittent/rare** (timing- or
heap/VFS-state-dependent, like the sibling `B-WAITQ-IDLEPARK` lost-wakeup
family) or was **already fixed** by an unrelated change since Part 47.
Because no root cause could be pinned and no deterministic repro exists,
the entry is downgraded from OPEN to **WATCH**.

A permanent **regression guard** now exists:
`self_test_linux_real_glibc_cc_brace_memset` (Path Z Part 56,
`kernel/src/proc/spawn.rs`), wired into the boot self-tests, compiles +
glibc-links + runs in ring 3 a program with a genuine runtime-`memset`
aggregate brace-initialiser and asserts output `42\n`. If tcc ever
regresses to losing `main` when a synthesised `memset` is present, that
rung fails and emits a `self-test failed` WARNING the boot-test scans
for. The field-init workaround in the other Path Z rungs is no longer
strictly required (brace-init is proven reliable) but is harmless and
left in place. The original OPEN analysis is retained below for history.

**Symptom.** A hosted compile+link in a *single* on-target tcc invocation
(`tcc -o /prog /prog.c`, the shape `run_hosted_cc_case` uses) fails with
`tcc: error: unresolved reference to 'main'` (exit 1) — even though the
source plainly defines `int main(void)`. The trigger observed live was an
aggregate **brace initialiser** (`struct s x = {…};`), which tcc lowers to
a synthesised `memset` reference; the *field-wise* version of the same
program (one fewer undefined symbol: only `write`) links and runs cleanly.

**IMPORTANT — earlier mechanism guess was wrong.** The first draft of this
entry blamed `libtcc1.a` (claiming tcc resolves the synthesised
`memset`/`memcpy` from its runtime archive and that perturbs the link).
That is **incorrect**: `ar t`/`nm --print-armap` on the staged
`libtcc1.a` show it defines the soft-float/atomic/alloca/va_list helpers
but **not** `memset`/`memcpy` — those resolve from glibc (`libc.so.6`).
So `libtcc1.a` is not pulled in by the brace-init program at all. The real
differentiator is simply the *one extra undefined symbol* (`memset`), and
the breakage is **on-target-specific**.

**Reproduction / diagnosis (what was actually done).** Extracted the whole
staged toolchain from `rootfs.ext4` via `debugfs -R "dump …"` (tcc, crt1/
crti/crtn.o, the `libc.so` GNU-ld GROUP script, `libc_nonshared.a`,
`libtcc1.a`, `libc.so.6`, `ld-linux`) and re-ran the *extracted target
tcc* under WSL:
  - `tcc -c prog.c -o prog.o` → OK; `nm` shows good `T main`, plus
    `U memset` for the brace-init variant vs. only `U write` for field-init.
  - Full `tcc -o prog prog.c` (one-shot compile+link) → **exit 0 for BOTH
    variants**, both with WSL's native crt/libc and with the OS's staged
    crt + `libc.so` GROUP script + `libc_nonshared.a` + `libtcc1.a` forced
    in explicitly via `-nostdlib`.
So the `unresolved 'main'` failure **does not reproduce off-target** — it
only happens when tcc runs *inside the OS* (under our Linux-syscall
translation + VFS). That points at an OS-side interaction (tcc's file
reads of the large `libc.so.6` / GROUP-script / archive parsing under our
syscall+VFS layer, or a heap/symbol-table quirk in tcc keyed to the extra
symbol), **not** an archive-index or link-ordering defect in the staged
files themselves.

**Why it matters.** The on-target C toolchain can currently mis-link (in
one step) programs that carry an extra compiler-synthesised undefined
symbol — most commonly aggregate brace initialisers (a lot of ordinary C).
Path Z rungs sidestep it (hand-rolled field init); coreutils/real projects
may hit it. Workaround: compile `-c` then link separately, or avoid the
construct.

**Where it lives.** On-target `tcc` (`/bin/tcc`, 0.9.28rc mob) running via
the Linux-ABI syscall translation + VFS; the staging is in
`stage_hosted_cc_support` (`kernel/src/proc/spawn.rs`). The self-test that
first exposed it: `self_test_linux_real_glibc_cc_struct` (Path Z Part 47).

**Proper fix (open — needs on-target instrumentation).** Because it only
reproduces inside the OS, the next step is to capture what tcc actually
does there: strace-equivalent of the failing link (there is already
`scripts/extract-tcc-strace.sh` / `scripts/probe-tcc-hosted.sh`) to see
whether a file read of `libc.so.6` / the GROUP script / `libc_nonshared.a`
returns short/EOF-early, or whether tcc's dynamic-symbol lookup for
`memset` walks into a region our VFS serves incorrectly. If a specific
syscall/VFS read is returning wrong data for large files under tcc's
access pattern, fix that; otherwise it may be a genuine tcc bug worth
patching in the port. Until then the entry stays WATCH/OPEN with the
field-init workaround in place.

### B-COMPLETION-TIMER-IRQ-DEADLOCK. Timer-softirq completion notify blocking-locks SCHED → same-CPU deadlock if it preempts a SCHED holder — ROOT-CAUSED & FIXED 2026-07-15

**Class.** Interrupt-reentrancy deadlock on the global `SCHED` (and `CP_TABLE`)
spinlock — the same broad family as B-SYSCTL-IRQ-DEADLOCK, and the strongly
suspected root cause of B-SCHED-SPAWN-DEADLOCK (see above).

**Root cause.** `ipc::timer::process_timer_expirations()` runs in the timer
**softirq** (`softirq::handle_timer`, interrupts enabled). For each expired timer
bound to a completion port it called `completion::notify()`, which takes the
blocking `CP_TABLE.lock()` and then the blocking `sched::wake()` →
`SCHED.lock()`. `SCHED` holders in `sched/mod.rs` do **not** disable interrupts,
so the timer softirq can fire on a CPU that is mid-critical-section holding
`SCHED` (or in the tiny acquire→record window). On the single-CPU boot the
softirq's `SCHED.lock()` then spins forever waiting for a lock the *same* CPU
holds — a self-deadlock. It only triggers when a completion-port timer expires
in that exact window, hence the ~1-in-20-to-28 rarity the soak observed.

**Audit scope (all clean except the one site).** Verified every other
softirq/IRQ-reachable path is already non-blocking on `SCHED`:
`#PF` handler (`try_resolve_fault`→`PROCESS_TABLE.try_lock`; `account_fault`,
`panic_diagnostics`→`SCHED.try_lock`); timer preemption (`do_deferred_preempt`
defers when `SCHED.is_locked()`); device-IRQ wake (`ioapic::handle_device_irq`
→`sched::try_wake`); `softirq::handle_timer` sub-calls
(`process_sleep_wakeups`→`wake_expired_sleeper` try_lock,
`process_deferred_wakes`→`try_wake`, `push_balance`→`SCHED.try_lock`);
`softirq::handle_sched`→`push_balance` (try_lock);
`ktimer::process_expirations` (defers callbacks to the workqueue, no inline
SCHED lock). Only `process_timer_expirations`→`completion::notify` blocked.

**Fix.** Added `completion::try_notify(cp, source) -> bool` (softirq-safe:
`CP_TABLE.try_lock()` + `sched::try_wake()`; commits **nothing** on contention —
returns `false` so the caller retries next tick, avoiding both a lost wakeup and
a duplicated event). Restructured `process_timer_expirations` to call
`try_notify` *before* advancing/expiring the timer and to `continue` (leave the
timer un-advanced, retry next ~10 ms tick) on contention. `completion::notify`
(blocking) is unchanged for its task/syscall-context callers (`io_ring`,
`syscall::handlers`) and `close()`. Contention is transient (SCHED/CP_TABLE held
only briefly), so the bounded per-tick retry resolves within a tick or two.

**Where it lives.** `kernel/src/ipc/completion.rs` (`try_notify`),
`kernel/src/ipc/timer.rs` (`process_timer_expirations`). Detector:
`scripts/wedge-soak.sh` (was catching it as B-SCHED-SPAWN-DEADLOCK).

**Next step.** Boot-test, then run a long `wedge-soak.sh` to confirm the SCHED
wedge no longer reproduces (it was ~1/20-28; a clean 40+ iteration soak is good
evidence). This is a confirmed 4th instance of the raw-`spin::Mutex` deadlock
class — see open-questions.md Q24 (recommendation was "escalate to C if a 3rd/4th
shows up"; this is the interrupt-reentrancy sub-variant, already fixed reactively
without a new lock type, consistent with the B-SYSCTL fix).

**UPDATE 2026-07-16 — CONFIRMED.** The confirmation soak
(`soak-20260715-235730`) ran 40 iterations: the SCHED spinloop wedge did **not**
reproduce in any of them (was ~1/20-28), strong evidence the fix holds. The soak
*did* stop on a **different, pre-existing** wedge at iter40 — a kernel jump to
`RIP=0x0` during the tcc-signal Path-Z self-test that cascaded into a kernel
stack-overflow storm. That is unrelated to this deadlock (different signature: a
control-flow hijack, not a spinloop) and is tracked separately as
**B-KNULLJUMP-SIGNAL** below. Downgrading this entry's confidence: the fix is
validated; leaving as ROOT-CAUSED & FIXED.

### B-SYSCTL-SPIN-MUTEX-PREEMPTED-HOLDER. kswapd spun forever on `sysctl::REGISTRY`, a bare `spin::Mutex` whose holder had been descheduled — 2026-08-13 — MECHANISM FIXED, TRIGGER STILL OPEN

**Caught by:** the 250-boot B-KNULLJUMP soak, iteration 20 of 20
(`build/knulljump-soak.log`; artifacts
`build/hang-catches/soak-20260813-061906-iter20.{serial,regs,stdout}.txt`).
19 of 20 boots were clean. **This is not B-KNULLJUMP** — that signature is
`RIP=0x0` with `error=0x10` (a control-flow hijack). Here `RIP` is a perfectly
real kernel address and the CPU is *executing normally*, just never finishing.

**What the evidence says.** `resolve-rip.sh` on the wedged RIP and the
rbp-chain the liveness dump walked:

```
0xffffffff80f32d56 -> core::sync::atomic::spin_loop_hint   (a bare `pause`)
  # 0: 0xffffffff80f1ab87 -> kernel::sysctl::get
  # 1: 0xffffffff80777ffe -> kernel::mm::kswapd::watermark_low
  # 2: 0xffffffff80777a63 -> kernel::mm::kswapd::kswapd_entry
  # 3: 0xffffffff807e2e79 -> task_entry_trampoline
```

All 16 RIP samples are the same instruction. The last boot-sequence line on
serial is `[kswapd] Background page reclaimer started` — the *very next*
statement in `kswapd_entry` is the `low_wm=…` `serial_println!`, whose first
argument is `watermark_low()`, i.e. `sysctl::get(PARAM_MM_MIN_FREE_PAGES)`.
kswapd wedged on its first ever sysctl read and never printed a second line.

Disassembling `sysctl::get` shows the spin is an *inlined* `spin::Mutex::lock`:
a `compare_exchange_weak` on the flag byte at `0x822cff28` (= `sysctl::REGISTRY`)
falling into a `load` + `pause` loop. The task table confirms the other half:
tid=0 (`prctl-batch269`, prio 31 — *higher* than kswapd's 20) sat **Ready** for
5562 ticks and was never picked, while `ctx_switches` stayed frozen at 1573 with
the heartbeat still climbing. So the timer was firing and the scheduler was
refusing to switch.

**Mechanism.** `kernel/src/sysctl.rs` had `use spin::Mutex;` — the *bare*
`spin::Mutex`, not `crate::sync::Mutex`. Three consequences, all of them bad:

1. **No `preempt_disable()` on acquire.** `crate::sync::Mutex::lock` disables
   preemption for the whole hold precisely because "a spinlock must never be
   held across a context switch". The bare lock has no such guard, so a holder
   can be descheduled mid-update and every later caller spins on a lock whose
   owner is Ready-but-not-running.
2. **No stall detector.** `crate::sync::Mutex::lock_contended` prints a one-shot
   diagnostic after `STALL_SECONDS` and keeps spinning. The bare lock spun for
   **five minutes** and printed nothing, which is why the hang looked like a
   silent freeze instead of naming itself.
3. **No owner tracking.** `crate::sync::Mutex` records the holding tid so
   `report_stall` can say *who*. The bare lock cannot, so the task table left
   "which task holds REGISTRY?" unanswerable.

**Fixed.** `sysctl::REGISTRY` is now
`crate::sync::Mutex::named(Registry::new(), b"sysctl-reg")`. That closes the
preempted-holder window outright and makes any future contention self-report
with a name and an owner. `list_all()` was also allocating its `Vec` *inside*
the critical section; with preemption now disabled for the hold that nests the
heap allocator (and, under pressure, reclaim) under a spinlock, so the
reservation moved above the `lock()` and the guard is explicitly dropped before
the return. The ISR contract is unchanged: interrupt-context readers still must
use `try_get` (see B-SYSCTL-IRQ-DEADLOCK).

**Still open — what froze `ctx_switches`.** The fix removes the *mechanism* by
which a sysctl holder could be preempted, but it does not fully explain why
CPU 0 stopped switching at all. Two candidates remain, and the evidence to date
cannot separate them:

* a leaked `preempt_disable()` somewhere on CPU 0 (`PREEMPT_DISABLE_COUNT` is
  **per-CPU**, so a task that voluntarily blocks while holding a tracked
  `crate::sync::Mutex` would leave the count elevated with nobody on-CPU to
  decrement it — preemption is then off for that CPU forever); or
* the run queue genuinely not holding tid=0, consistent with
  `local_has_real_work=false` printed beside a Ready prio-31 task.

To tell them apart on the next catch, the liveness dump now prints
`preempt_disable_depth=` per CPU (`kernel/src/sched/mod.rs`, from the existing
`preempt_count(cpu)`). A non-zero depth means "the scheduler was not *allowed*
to switch"; zero means "the scheduler *chose* not to", which is a completely
different bug. Re-run the soak and read that field first.

**Context worth keeping.** The wedged boot was also in an IRQ 11 storm
(~600 000 IRQs/sec, four mask/cooldown cycles) and had just come through the
OOM self-test driving memory pressure to `critical`. Whether the storm is a
cause, a consequence, or a coincidence is unknown; it is recorded here so a
repeat catch can be compared.

### B-VIRTIO-BLK-WRITE-TIMEOUT. Intermittent boot hang — a spurious virtio-blk request timeout corrupts the virtqueue, cascading into an unrecoverable storm of write timeouts during ext4 journal replay — ROOT-CAUSED & FIXED 2026-07-15

**Symptom.** A live boot wedge caught by `scripts/wedge-soak.sh`
(`build/hang-catches/soak-20260715-190010-iter28.*`), ~1 in 28 boots.
Distinct from the three prior soak catches (B-WAITQ-IDLEPARK,
B-SCHED-SPAWN-DEADLOCK, B-SYSCTL-IRQ-DEADLOCK) — the SCHED/idle-fallback
dumps correctly did **not** fire. The i6300esb NMI watchdog froze CPU#0 at
`RIP=0xffffffff81e9492a` = `ext4::journal::Journal::open`, `RFL=0x86`
(IF *set* — not a spinlock deadlock; the CPU is livelocked retrying I/O).
The serial log ends with **136** `[virtio-blk] Write sector N timed out`
messages (first in polling mode, later in IRQ mode) interleaved with a
livelock of `[sched] Anti-starvation: cur=0 boosted 1 task to priority 0:
[130(p20)]` (kswapd starved while the boot task spins retrying journal
writes).

**Root cause.** Two compounding bugs in the single-outstanding virtio-blk
driver (`kernel/src/virtio/blk.rs`), which shares *one* DMA frame across all
requests:
1. **Trigger — too-short polling budget.** `wait_completion`'s polling
   fallback (early boot, pre-IOAPIC) timed out after only `1_000_000`
   `spin_loop()` iterations (~1 ms). Under soak-test host contention a real
   QEMU virtio-blk completion can take longer, so the *first* timeout fired
   spuriously even though the device was healthy and would have completed.
2. **Cascade — unsafe timeout recovery.** On timeout the old code did
   `self.queue.free_chain(head)` and returned `Err`, but the device **still
   owned** those descriptors and the shared DMA buffer. The caller
   (`Journal::open`) retried; the next `submit()` reused the just-freed
   descriptors and the same DMA buffer. When the device finally completed
   the abandoned request, `poll_used()` returned that stale head (the driver
   accepted *any* completion with no head-matching), the used ring desynced,
   and `free_chain` double-freed a descriptor — corrupting the free list.
   From there every request timed out (now in IRQ mode, 5 s each), an
   unrecoverable storm.

**Fix (this session).**
- **Adequate polling budget.** New `POLL_TIMEOUT_SPINS = 100_000_000`
  constant (100× headroom) so a healthy device under load never spuriously
  times out, while still bounding a genuinely-dead device so boot can't hang
  forever.
- **Head-matching completion.** New `poll_matching(head, …)` only returns
  when the completion's head equals *our* submitted head; a mismatched
  (stale) completion is drained (`free_chain`) and polling continues.
  Guarantees `read_sector`/`write_sector` free exactly the chain they
  submitted.
- **Safe timeout recovery.** On timeout the driver no longer blindly frees a
  device-owned chain. Instead `recover_after_timeout()` → `recover()`
  re-runs the legacy virtio init handshake (reset → ACK → DRIVER → features
  → re-select queue 0 → `Virtqueue::reset()` → re-publish queue PFN →
  DRIVER_OK), forcing the device to relinquish **all** outstanding buffers
  so the next request starts from a clean, consistent state. New
  `Virtqueue::reset()` (`kernel/src/virtio/queue.rs`) re-zeroes the rings,
  rebuilds the free list, and clears avail/used index tracking, reusing the
  same backing frame.

**Status.** FIXED. The spurious-timeout trigger is removed and, even if a
timeout does occur (genuinely dead device), the reset-based recovery keeps
the virtqueue consistent instead of cascading. Re-soak to confirm the wedge
no longer reproduces.

### B-SYSCTL-IRQ-DEADLOCK. `sysctl::REGISTRY` (raw `spin::Mutex`) acquired blockingly from interrupt context → single-CPU hard deadlock — ROOT-CAUSED & FIXED 2026-07-15

**Symptom.** A live boot wedge caught by `scripts/wedge-soak.sh` (iter 4).
The i6300esb NMI hard-lockup watchdog captured a frozen guest with
`RIP=0xffffffff81acd516` (`spin_loop_hint`) and `RFL=0x00000002` (IF
cleared → interrupts disabled while spinning, i.e. a spinlock deadlock,
not a lost-wakeup/idle bug). The NMI backtrace showed
`serial::_print ← sysctl::set ← mm::oom::self_test`, with
`RDI = &sysctl::REGISTRY`; a stack scan additionally showed
`timer_tick → check_starvation → sysctl::get` frames.

**Root cause.** `static REGISTRY: spin::Mutex<Registry>` (kernel/src/sysctl.rs)
is a *raw* `spin::Mutex` — it does **not** mask hardware interrupts on
acquire. It was reachable from two contexts:
  - **Task context:** `sysctl::set()` held `REGISTRY` across a slow
    `serial_println!` (the `[sysctl] name = v (was old)` log).
  - **Interrupt context:** the timer IRQ's `sched::check_starvation()`
    (sched/mod.rs) called the *blocking* `sysctl::get()` to read
    `sched.starvation_threshold`; the #PF stack-grow handler (idt.rs)
    likewise called blocking `sysctl::get()` for `mm.max_stack_frames`.
On a single CPU, when the timer IRQ fired while a task held `REGISTRY`
(inside `set()`'s log window), the ISR spun on `REGISTRY.lock()` forever
— the interrupted holder can never resume to release it. Classic Q24
"raw spin::Mutex holder-preemption / interrupt-reentrancy" deadlock
(same class as the already-fixed heap-lock 83307bdfc and container::TABLE
fa87bbb5e).

**Fix (proper).**
  1. Added `sysctl::try_get(id) -> Option<u64>` — a non-blocking read
     using `REGISTRY.try_lock()`, returning `None` on contention so the
     caller falls back to its compile-time default (always safe for these
     tunables). Interrupt/exception-context readers MUST use this, never
     the blocking `get()`. (Mirrors how `check_starvation` already uses
     `SCHED.try_lock()`.)
  2. Converted the two IRQ/exception-context callers to `try_get`:
     `sched::check_starvation` (sched/mod.rs) and the #PF stack-grow
     handler (idt.rs).
  3. Stopped `sysctl::set()` from holding `REGISTRY` across the log: it
     now snapshots an owned `ParamInfo` via
     `let info = REGISTRY.lock().find(id);` (guard drops at the `;`) and
     logs lock-free, closing the window entirely.

The remaining `sysctl::get()` callers (frame-alloc slow path, kswapd,
oom self-test, swap, syscall handlers, procfs) all run in task/syscall
context and are fine keeping the blocking read.

**Repro (pre-fix).** `scripts/wedge-soak.sh` (hard-lockup watchdog); the
wedge appeared within a handful of iterations under the oom/container
self-test load that exercises `sysctl::set`.

**Validation (post-fix).** 6/6 wedge-soak iterations booted to BOOT_OK
(101–158s each) with zero wedges caught (2026-07-15). NOTE: validating
this required first fixing a separate harness bug — boot-test.sh was
leaking orphaned native QEMU processes on Windows (MSYS `kill` does not
reap them), which locked `serial-test.txt` and made every repeated soak
iteration fast-fail. Fixed in the same session via `-pidfile` +
`taskkill` (commit 845c4447b); the sysctl fix itself is 0da3324e5.

**Proactive audit of the whole bug class (2026-07-15).** Since this was
the *third* found instance of a raw `spin::Mutex` deadlocking across the
task/IRQ boundary (prior two: heap lock 83307bdfc, `container::TABLE`
fa87bbb5e), I audited the two highest-risk interrupt/exception entry
paths for the same pattern rather than waiting for the next one to wedge
a boot. The invariant every IRQ-reachable lock must satisfy: EITHER the
IRQ-context reader uses `try_lock` (fall back to a default on
contention), OR *every* task-context holder wraps the lock in
`crate::cpu::without_interrupts` (masks IRQs, not just preemption — the
preempt-aware `crate::sync::Mutex` alone is insufficient because it does
not clear IF).
  - **Timer hard-IRQ path** (`apic::handle_timer_irq`, IF=0):
    `sched::timer_tick` uses `SCHED.try_lock()`; `check_starvation` now
    uses `sysctl::try_get` (this fix); `cgroup::{cpu_charge,
    cpu_period_reset, io_period_reset}` all use `TABLE.try_lock()`;
    `hrtimer::{process_expired, next_expiry_ns}` and every task-side
    `hrtimer` lock (`schedule_absolute`, `cancel`, `pending_count`) use
    `without_interrupts`. All clean.
  - **Page-fault exception handler** (`idt::handle_page_fault`): body
    takes no direct spin lock (grep for `.lock()` from its entry = none)
    beyond the `sysctl::get`→`try_get` stack-frame-limit read fixed here;
    it delegates to mm helpers that own their locking.
  - **Device IRQs** route through `ioapic::handle_device_irq` and defer
    to userspace drivers via the IRQ-poll softirq (bottom half, IF=1),
    so they are not on the IF=0 hard-IRQ deadlock path.
  Conclusion: the sysctl case was an isolated oversight; the hot IRQ
  paths are otherwise correctly disciplined. A future session extending
  IRQ-context code must preserve the try_lock-or-without_interrupts
  invariant above.

---

### B-PTHREAD-JOIN-LOST-CTID. `pthread_join` intermittently blocks forever because the child's `CLONE_CHILD_CLEARTID` was registered *after* the child was made runnable — FIXED 2026-08-13

**Symptom.** The Path-Z real-glibc pthread self-test times out rather than
producing a wrong answer (this is a *different* failure from
B-PTHREAD-CHILD-JUMPS-TO-GARBAGE below, which returns wrong counters):

```
[spawn]   FAIL: real glibc pthread — process did not exit within 262144 yields
          (state=Some(Running)); a thread likely deadlocked on a futex or a worker faulted
WARNING: Path-Z real glibc pthread self-test failed: TimedOut
```

Caught on iteration 1 of soak run `soak-20260813-222924`
(`build/hang-catches/soak-20260813-222924-iter01.serial.txt`), i.e. at roughly
the same ~1-in-10 rate as the other pthread flakes.

**Reading the serial log — how to tell this apart from a worker fault.** The
region around the failure is:

```
[sched] Spawned task 265 ...            <- main thread of process 296
[sched] Spawned task 266 ...
[sched] Task 266 exiting                <- worker exits IMMEDIATELY after spawn
[sched] Spawned task 267 / 268 / 269
[sched] Task 267 exiting
[sched] Task 268 exiting
[sched] Task 269 exiting
[thread] Process 296 has no threads left — now zombie
[spawn]   FAIL: ... (state=Some(Running))
```

All four workers exit cleanly and none faults, yet the main thread 265 never
prints `[sched] Task 265 exiting`. Two details are easy to misread:

* The `now zombie` line is **not** task 265 exiting. It comes from the test's
  own cleanup, `thread::on_thread_exit(result.task_id)` at
  `kernel/src/proc/spawn.rs` (in the glibc-pthread test, just after the state
  snapshot), which removes the still-registered main thread. That is also why
  the `FAIL` line can report `state=Some(Running)` on the line *after* a
  `now zombie` message — the state was sampled before that cleanup ran.
* In a healthy boot the order is `... 269 exiting` → `now zombie` →
  `Task 265 exiting`, because `sys_exit` calls `on_thread_exit` (which prints
  the zombie line) *before* `sched::task_exit` (which prints the exit line).
  Compare `soak-20260813-093459-iter02/03/09` for the healthy shape.

So the signature is precisely: **every worker exits, the joiner never does.**

**Root cause.** `proc::thread_clone::clone_thread` called
`thread::spawn_with_tls(...)`, which *admits* the child (makes it runnable)
before returning, and only then ran:

```rust
let task_id = thread::spawn_with_tls(...)?;   // <-- child is RUNNABLE here
...
if (args.flags & CLONE_CHILD_CLEARTID) != 0 && args.child_tid_ptr != 0 {
    register_clear_child_tid(task_id, args.child_tid_ptr);   // <-- too late
}
```

On the uniprocessor a timer preemption in that window lets the child run to
completion. `on_thread_exit_hook` then does
`match CLEAR_CHILD_TID.lock().remove(&task_id) { Some(p) => p, None => return }`
— it takes the `None` path and performs **neither** the zero-write to `*ctid`
**nor** the `futex_wake(ctid_ptr, 1)`. glibc's `pthread_join` is parked on
exactly that word, so it blocks forever on a tid that will never be cleared;
the process never zombifies and the test burns its whole yield budget. The
late `register_clear_child_tid` also left a permanent `CLEAR_CHILD_TID` entry
keyed on a task id that had already exited (a slow leak, and a latent mis-fire
if task ids are ever recycled).

`CLONE_CHILD_SETTID` had the same window with an inverted effect: had the
child exited first, the parent's `copy_to_user` of the tid would have landed
*after* the exit hook zeroed `*ctid`, resurrecting a non-zero tid into a dead
thread's descriptor — hanging `pthread_join` just as surely.

`proc::fork::fork` carried an identical copy of the defect (its
`CLONE_CHILD_CLEARTID` registration also ran after `spawn_with_tls` returned).

**Why the futex layer was *not* at fault.** Two plausible-looking alternatives
were checked and cleared, so a future session need not redo them:
`futex_wait_bitset` performs its `*addr == expected` compare and its wait-queue
enqueue under one `FUTEX_TABLE.lock()`, so a wake cannot slip between the
compare and the enqueue; and the window between dropping that lock and calling
`sched::block_current()` is covered by the scheduler's sticky `pending_wake`
flag (`sched::wake` sets it when the target is Running/Ready, and
`block_current` consumes it and returns without blocking).

**Fix.** The set of per-thread state that must exist before a child can run had
grown to four items (`THREAD_OWNERS`, `pcb::add_thread`, the `%fs`/`%gs` bases,
and the ctid), and each one registered after the spawn call re-opened the same
race — so the two-phase spawn is now explicit at the `proc::thread` level
rather than being threaded through an ever-widening parameter list:

* `thread::spawn_suspended_with_tls(...)` — phase 1, returns a task that is
  registered with its process but still `Blocked`, so it cannot run or exit.
* `thread::admit(pid, task_id)` — phase 2, makes it runnable; unwinds the
  thread registration and destroys the task on failure.
* `thread::spawn` / `thread::spawn_with_tls` are now just phase 1 + phase 2 and
  are unchanged for the many callers that need no extra registration.
* `thread_clone::forget_clear_child_tid(task_id)` drops a ctid registration
  without firing it, used to unwind a failed `admit`.

`clone_thread` and `fork` now do all of PARENT_SETTID / CHILD_SETTID /
CHILD_CLEARTID between the two phases, so there is no instant at which the
child is schedulable with incomplete exit-path state.

**Files.** `kernel/src/proc/thread.rs` (the two-phase API),
`kernel/src/proc/thread_clone.rs` (`clone_thread`, `forget_clear_child_tid`),
`kernel/src/proc/fork.rs` (`fork`).

**Bug class — worth grepping for.** This is the third instance of
"register-after-admit" in the same code path: B-PTHREAD-YIELDBUDGET
(`THREAD_OWNERS`/`add_thread`), B-PTHREAD-CHILD-JUMPS-TO-GARBAGE defect 1
(`%fs` base), and now the ctid. **Any** new per-thread registration keyed on a
task id must go between `spawn_suspended_with_tls` and `admit` — never after
the spawn helper returns.

---

### B-THREAD-JOIN-EXIT-RACE. `thread::join` could park forever on a thread that exited between the liveness check and the waiter registration — FIXED 2026-08-13

**How it was found.** Not from a failing boot: it came out of the audit that
followed B-PTHREAD-JOIN-LOST-CTID above, checking every piece of state
`on_thread_exit` consults for the same "the other side registers too late"
shape. `THREAD_JOIN_WAITERS` is the one such table registered by the *joiner*
rather than by the spawner, so it needed the mirror-image argument.

**The window.** `proc::thread::join` did:

1. take `THREAD_OWNERS`, confirm the target exists and shares our process,
   **drop the lock**;
2. take `THREAD_JOIN_WAITERS`, insert `target -> caller`, drop it;
3. park in a loop whose *only* exit condition is that entry being removed.

`on_thread_exit` drains `THREAD_JOIN_WAITERS` (waking the joiner) **before** it
removes the task from `THREAD_OWNERS`. So a target that exits between steps 1
and 2 drains an empty waiter map, and the entry inserted at step 2 is one that
nothing will ever remove — the loop at step 3 never terminates and the joiner
is stuck for the life of the process.

This is a genuine hang, but a much narrower one than the ctid bug: it needs a
preemption inside a two-instruction-wide gap between dropping one lock and
taking another, and it only affects the native `SYS_THREAD_JOIN` path (glibc
`pthread_join` goes through the ctid futex instead), which is why no soak has
caught it.

**Fix.** Register-then-recheck, the same idiom `futex_wait_bitset` already uses
for the signal-arrival race. After publishing the waiter entry, re-check
`THREAD_OWNERS`; if the target is gone, the exit already ran, so withdraw the
entry (only if it is still ours — a racing `on_thread_exit` may have removed it
and issued a real wake, which must not be swallowed) and return the recorded
outcome, or `NoSuchProcess` if none — matching what the pre-existing
target-not-registered branch above already returns. The ordering that makes
this correct is the one noted above: waiters are drained strictly before the
`THREAD_OWNERS` removal, so "gone from `THREAD_OWNERS`" implies "the waiter
drain has already happened."

Neither lock is ever held while taking the other, so no new lock-order edge is
introduced.

**Files.** `kernel/src/proc/thread.rs` (`join`).

---

### TD-KERNEL-KILL-THREAD-DEAD-CODE. `proc::thread::kill_thread` has no callers — ✅ RESOLVED 2026-08-14 (and it was not dead code: it was an unwired bug fix) 2026-08-13

`kernel/src/proc/thread.rs:kill_thread` is a `pub fn` with zero call sites
anywhere in the tree, so the binary build emits a `dead_code` warning for it.
Noticed while building the two fixes above; deliberately left alone rather than
churned, because it is plausibly intended as part of the thread-teardown API
surface. Proper resolution: either wire it into the process-teardown path that
should be using it, or delete it and let `sched::kill_task` remain the single
entry point. Pre-existing — it is not a regression from those fixes (verified:
the same single occurrence, definition only, exists at commit `315a7e0ca`).

**Resolution 2026-08-14 — this was filed under the wrong heading. It was a
live bug, not tech debt.** The "either wire it up or delete it" framing above
treats the two options as comparable. They were not, and the answer was
written inside the function's own doc comment the whole time:

> Calling `sched::kill_task` on its own — which is what the shell's `kill`
> command **used to** do — skips every one of those and leaves the thread
> registered forever with its joiner parked.

The shell's `kill` command did not "used to" do that. It still did:
`kshell.rs:cmd_kill` called `crate::sched::kill_task(task_id)` directly. So
`kill_thread` was not a speculative API-surface addition — it was the fix for
that exact defect, written but **never wired to its one caller**. Every
`kill <tid>` typed at the kernel prompt therefore:

1. leaked a `THREAD_OWNERS` entry (the thread→process mapping is never
   removed, so the process can never reach its zombie transition through that
   thread);
2. left any task parked in `join()` on the victim parked forever; and
3. recorded no `ThreadOutcome::Killed`, so a joiner that *was* woken by some
   other path would read a fabricated normal return instead of
   `KernelError::Cancelled` — the precise wrong-answer failure mode that
   `B-PTHREAD-CHILD-JUMPS-TO-GARBAGE` defect 2 was fixed to prevent.

**Fix:** `cmd_kill` now calls `proc::thread::kill_thread`, with a doc comment
recording *why* it must not use the scheduler call directly (so the next
person to "simplify" it has the reason in front of them).

**Why nothing caught it.** There was no test of `kill_thread` at all — the
existing `test_killed_thread_does_not_join_normally` exercises `record_killed`
and `join` directly with synthetic task IDs, which covers the outcome map but
deliberately never calls `kill_thread`. So the only evidence the function had
no callers was a `dead_code` warning, and a warning that says "unused" reads
as "harmless" rather than "your fix is not connected".

**Test added:** `thread::test_kill_thread_cleans_up` (test 9). It spawns a
victim and kills it **before it is ever scheduled**, which (a) makes the test
unable to leave a runaway task behind if the kill is refused, and (b) lets the
entry function's untouched counter prove the task never ran, so the stack
`&counter` it was handed cannot outlive the frame. It asserts the accepted
kill deregisters the thread and joins as `Cancelled`, and that a *second*,
refused kill withdraws the speculative `Killed` marker (the `else` branch that
exists only in `kill_thread`) so a later join reports `NoSuchProcess` rather
than a death that never happened.

**Deliberately left alone:** `syscall/handlers.rs:5877` also calls
`sched::kill_task` directly, on the sibling threads of a process being torn
down by a fatal signal. That site pairs it with an unconditional
`thread::on_thread_exit(t)`, which `kill_thread` would skip for a thread that
is already dead — and for a dying process, deregistering an already-dead
sibling is the wanted behaviour. Its missing `record_killed` is immaterial
there because the joiners are inside the same process and are being killed in
the same loop.

---

### B-PTHREAD-CHILD-JUMPS-TO-GARBAGE. One `pthread_create`d thread intermittently starts at a bogus RIP and is killed; the process keeps running and reports a wrong answer — FIXED (defect 2 fixed 2026-08-13 `315a7e0ca`; defect 1 fixed 2026-08-13 `975114f54`, corroborated by a 20/20 clean soak) 2026-08-13

**Symptom.** A deliberate 40-boot soak (`scripts/wedge-soak.sh`, run
`soak-20260813-093459`) was launched to hunt an unrelated wedge. It did not
find the wedge; it found this instead, on iteration 10 of 10 completed
(iterations 1–9 clean, so the measured rate is **1 failure in 10 boots**).
The Path-Z real-glibc pthread self-test produced:

```
captured: SLATE_GLIBC_PTHREAD_OK counter=30000 joinsum=9
expected: SLATE_GLIBC_PTHREAD_OK counter=40000 joinsum=10
```

**What those numbers mean.** The test binary (built by
`scripts/create-ext4-rootfs.sh`, the `pthread.c` heredoc) creates 4 threads;
worker `i` does 10 000 mutex-guarded `counter += 1` and returns `i + 1`, so a
correct run is always `counter=40000 joinsum=10`. `40000 - 30000 = 10000` and
`10 - 9 = 1` identify the casualty exactly: **the thread with `id == 0`** —
the first one created — contributed neither its increments nor its return
value. Nothing else diverged.

**The kill.** From the serial log
(`build/hang-catches/soak-20260813-093459-iter10.serial.txt`, lines ~19256–19280;
`build/` is gitignored, so the excerpt is reproduced here):

```
[sched] Spawned task 266 (priority 16, cpu 0)      <- worker id 0
[mmap] Lazy mapped 0x6000a1a000..0x600121e000 (513 frames, demand-paged)
[sched] Spawned task 267 (priority 16, cpu 0)
...
[sched] Task 267 exiting
[sched] Task 268 exiting
[sched] Task 269 exiting
[exception] User page fault (task 266) at 0x600005eff0, addr=0x600005eff0 (not-present, read) — trying SEH
[exception] Killing task 266 — Page Fault (#PF) at 0x600005eff0 (ring 3)
  CS=0x23 RFLAGS=0x10216 RSP=0x6000a15788 SS=0x1b
[exception] Recording crash: pid=296 exception=8 rip=0x600005eff0 aux=0x600005eff0
```

**`rip == aux == CR2`** — the faulting address *is* the instruction pointer.
Task 266 did not deref a bad pointer; it **jumped to** one. And 0x600005eff0
is not in either loaded image (the binary is at bias 0x57ffc1e4c000, the
loader/libc at 0x72c7a9914000) — it is an address in the low mmap arena,
*below* every region this process lazily mapped (the lowest logged is
0x6000212000). Its `RSP=0x6000a15788` is correctly inside its own thread
stack (0x6000216000..0x6000a1a000), so the stack pointer survived; only the
control transfer went wrong.

**Reading of the mechanism (unconfirmed).** glibc's `start_thread` reads
`pd->start_routine` out of the thread descriptor via `%fs`. A garbage value
there — because `CLONE_SETTLS` installed the wrong `%fs` base for this child,
or because the child was made runnable before the parent's descriptor stores
were visible to it — produces exactly this: a jump to an arena-looking
address with an otherwise intact stack. That the victim is always(?) the
*first* child, while children 2–4 ran and exited normally, points at a
first-time-through / setup-ordering window rather than steady-state
contention. Confirming this needs the child's `%fs` base and the descriptor
contents logged at `clone` time — see below.

**Two separate defects are visible here, and the second one is arguably worse:**

1. The thread jumped to garbage (above). **Still OPEN** — see "Next step"
   below.
2. **The process did not die, and reported a plausible-looking wrong answer.**
   `pthread_join` on the killed thread returned success with `ret == NULL`,
   which is how `joinsum` became 9 instead of 10. On Linux a `SIGSEGV` in any
   thread terminates the whole process; here only the thread was killed, the
   remaining threads finished, and `main` printed a result that looks like a
   normal run. A test that asserted only "the binary exited 13" would have
   passed. Whatever the fix for (1), the kill path must not let a
   fault-killed thread be joined as if it had returned normally — a
   `pthread_join` on a thread the kernel killed should be distinguishable, and
   a ring-3 fault with no SEH handler should take down the process, not one
   thread of it.
   **FIXED 2026-08-13.** Three changes, all of which had to land together:

   - `kernel/src/idt.rs::kill_userspace_task_with_info` now calls
     `proc::thread::kill_process_threads(pid)` instead of
     `on_thread_exit(task_id)`: an unhandled ring-3 fault — one that both
     the Linux-ABI signal path (`try_deliver_linux_fault_signal`) and the
     native SEH trampoline (`try_dispatch_user_exception`) declined —
     takes down the **whole process**, which is the default disposition
     under both Windows SEH and Linux `SIGSEGV`. `kill_process_threads`
     subsumes the old `on_thread_exit` for the faulting task itself
     (`sched::kill_task` refuses the *current* task, which is
     `task_exit`'s job, but the thread→process mapping is still dropped).
   - `kernel/src/proc/thread.rs` replaces `THREAD_EXIT_VALUES:
     BTreeMap<TaskId, i64>` with `THREAD_OUTCOMES: BTreeMap<TaskId,
     ThreadOutcome>`, where `ThreadOutcome` is `Exited(i64) | Killed`.
     `join()` used to report `Ok(0)` for *any* thread that ended without
     recording a value — which is exactly what a killed thread looks like
     — so a dead worker's contribution silently vanished into a zero.
     Every involuntary death path now calls `record_killed()` **before**
     `on_thread_exit` (which is what releases a parked joiner, so a marker
     written afterwards can arrive too late), and `join()` reports
     `KernelError::Cancelled`. Reaching the "no outcome at all" case now
     means the caller joined a *detached* thread, which is `EINVAL`, not a
     silent zero. New `proc::thread` self-test 8
     (`test_killed_thread_does_not_join_normally`) locks this in.
   - `test_blocking_join`'s second phase had to change with the semantics,
     and the way it failed is worth recording: it models a thread that dies
     *without* passing through `thread_exit_with_value` (a crash), and it
     asserted the old `Ok(0)`. The first boot after the fix duly printed
     `FAIL: join() returned -9223372036854775808` — `i64::MIN` being the
     joiner's `unwrap_or` sentinel for "join returned `Err`". The phase now
     stamps `record_killed()` on the target, which is what the real crash
     path does, and expects `Cancelled`. The joiner also had to stop
     folding the error into the value (`join(t).unwrap_or(i64::MIN)`) and
     publish the value and the error discriminant in separate atomics —
     the same value/error ambiguity that forced the `SYS_THREAD_JOIN` ABI
     change, reproduced in miniature in the test harness.
   - `SYS_THREAD_JOIN` (512) changed shape: the exit value now travels
     through an `arg1` out-pointer and the syscall returns `0`/`-errno`.
     The old value-in-rax ABI **could not represent the fix**: a thread may
     exit with a legitimately negative value — `pthread_exit(PTHREAD_CANCELED)`
     is `(void *)-1`, and `Cancelled` is `-5` — so an exit value and an
     error code were indistinguishable. `posix`'s `pthread_join` passes a
     stack slot, and maps `Cancelled` to a *successful* join returning
     `PTHREAD_CANCELED`, which is precisely the slot POSIX reserves for
     "this thread did not finish normally".

   Also fixed in passing: kshell's `kill` command called `sched::kill_task`
   directly, which only marks the scheduler task Dead — leaving the
   thread→process mapping registered, IRQ registrations dangling and any
   joiner parked forever. It now goes through the new
   `proc::thread::kill_thread()`, which records the kill, kills the task
   and runs the universal death hook.

   Note the failing fixture above reaches `pthread_join` through *glibc's*
   futex-based join over the Linux ABI, not through `SYS_THREAD_JOIN`, so
   for that test it is the `idt.rs` half that makes the difference: the
   process now dies instead of printing `joinsum=9`.

**Distinct from the neighbouring pthread entries.** B-PTHREAD-TEARDOWN-PF
(below) is a *kernel*-mode `#PF` at a near-null address during *teardown*;
this is a *ring-3* fault at *startup*, and the kernel itself stays healthy.
B-PTHREAD-YIELDBUDGET (resolved) was a silent hang, not a fault.

**Reproduce.** `bash scripts/wedge-soak.sh` (or plain repeated
`scripts/boot-test.sh`) — expect roughly one failure per ten boots. The
soak script already treats a self-test regression as a catch and preserves
the serial log, which is how this was captured.

**Next step when picked up (defect 1).** ~~Add a `clone`-time trace to
`kernel/src/proc/thread_clone.rs` printing, for each child: the requested TLS
base, the `%fs` base actually installed, and the first 8 bytes at
`tls_base + offsetof(struct pthread, start_routine)`; then soak until it trips.
The failure is frequent enough (1/10) that a single 20-boot soak should catch
it with the trace attached.~~ **Superseded — the defect was found by static
audit instead, see below.**

**Defect 1 FIXED 2026-08-13** (`975114f54`, *seed thread `%fs`/`%gs` base
before admission*). The planned trace was never needed: auditing the spawn
path for the register-after-admit pattern found the mechanism directly.

`clone_thread` called `thread::spawn_with_tls`, which **admitted the child to
the run queue before writing its `%fs`/`%gs` base**. On our uniprocessor
(TCG) build a timer preemption inside that window lets the child start with an
unseeded `%fs`. glibc's clone entry stub loads the thread function from
TLS — a `%fs`-relative fetch of `struct pthread`'s `start_routine` — so with a
stale/zero `%fs` base it reads a garbage word and jumps to it. That is exactly
the reported signature: worker `id == 0` (the first child created, i.e. the one
most likely to be preempted before seeding) starting at a bogus RIP with
`rip == aux == CR2`, the fault address *being* the instruction pointer.

Fixed structurally rather than by reordering two statements: `thread` now
exposes a two-phase API — `spawn_suspended_with_tls()` (create + register
everything, including the TLS bases) followed by an explicit `admit()` — so a
child cannot become runnable before its per-thread state exists.
`spawn_with_tls` is retained as a thin wrapper that calls both.

**Confirmation and its honest limits.** The 20-boot soak this entry asked for
has since run (`build/hang-catches/soak-ctidfix.log`, 2026-08-13 23:02 →
2026-08-14 01:52): **20/20 boots passed**, every one reporting
`REAL glibc pthread (… 40000 mutex/futex ops, pthread_join, captured 48 bytes
== expected): OK` — i.e. the exact `counter=40000 joinsum=10` assertion whose
failure defined this bug — and zero kernel faults.

That is consistent with a fix but is **not** statistically conclusive on its
own: at the measured 1-in-10 rate, 20 clean boots would happen by chance
`0.9^20 ≈ 12%` of the time. The confidence comes primarily from the mechanism
being understood and closed by construction, with the soak as corroboration.
If a `counter=30000 joinsum=9` (or the now-loud faulting variant) ever
reappears, reopen this entry rather than assuming a new bug.

**Bug class.** Third of three register-after-admit defects found in this
subsystem, alongside B-PTHREAD-JOIN-LOST-CTID (the ctid registration) and
B-THREAD-JOIN-EXIT-RACE (the join-waiter registration). Worth grepping for
whenever new per-thread state is keyed on a task id.

**How the defect-2 fix changes what a soak looks like.** Before, the 1-in-10
boot that hit this produced a *quiet wrong answer* (`counter=30000
joinsum=9`) that only the self-test's exact-value assertion caught. Now the
process dies on the fault, so the same race surfaces as a loud failure. That
is the intended consequence, not a regression — and it is exactly the signal
the trace above needs.

---

### B-FAULT-SERIALSTORM. Unconditional per-page-fault `serial_println!` saturated the (slow) serial port during demand-paging bursts, starving the hard-lockup kick and making boots crawl / appear hung — FIXED 2026-07-14

**Where:** `kernel/src/proc/pcb.rs` — `try_resolve_fault` (demand-paged
anonymous frame site, ~L5267) and `resolve_file_cached` (page-cache mapped
site, ~L5352).

**Symptom / how it was found:** while validating the i6300esb NMI
hard-lockup watchdog (Q20/§61, `boot-test.sh --hard-lockup-watchdog`), a
boot ran ~4915 ms/stage behind and the NMI fired on ~9.7 s of BSP
kick-starvation:
```
[hardlockup] armed (NMI on ~9.8s BSP silence)
[sched] Task [hardlockup] NMI WATCHDOG FIRED cpu=0 rip=0xffffffff8010f556 ...
        heartbeat=5365 kick_stale_ns=9738940603 — dumping backtrace + task table
```
The captured `rip`/rbp-chain, re-resolved with exact 64-bit integer
arithmetic (awk's double precision silently zeroed the high bits of
`0xffffffff8010f556`), walked through `spin_loop_hint` →
`liveness_boot_deadline_check` → `timer_tick` — i.e. the BSP was *not*
deadlocked, it was simply spending all its time emitting serial. Each
demand-paged frame and each page-cache mapping printed an unconditional
`serial_println!`; a process faulting in its whole address space emits
thousands of these, and the 115200-baud serial port (~11 KB/s) cannot
drain them fast enough. The write path back-pressures in kernel context,
delaying `hardlockup::kick()` from `timer_tick` past the watchdog's
~9.8 s threshold — the boot looked hung and, under host load, could
tip the documented B-DASH-STDIN-FLAKE reap race over its own edge.

**Fix:** route both hot-path fault logs through
`crate::klog!(Trace, "mm.fault", …)` instead of `serial_println!`. klog's
`serial_level` defaults to `Info`, so Trace entries stay in the dmesg ring
buffer (still available for debugging via `dmesg`) but are kept OFF serial
by default. No fault-path log is lost; only the serial storm is gone.

**Validation:** `boot-test.sh` after the fix reached `BOOT_OK` in 132 s
with `storm=0` (zero `Demand-paged`/`Page-cache mapped` serial lines vs.
thousands before) and the container multi-network self-test still passing.
Boot no longer crawls; the hard-lockup kick is no longer starved by
demand-paging bursts.

**Note (Q20 watchdog validated):** this capture also *confirms the
i6300esb NMI hard-lockup detector works end-to-end* — it armed over the
boot ring-3 window, detected real BSP kick-starvation, delivered an NMI on
the dedicated IST2 stack, and dumped a usable rbp-chain backtrace + task
table exactly as designed. The detector doing its job is what surfaced
B-FAULT-SERIALSTORM in the first place.

### B-PREEMPT-SPINLOCK. Involuntary preemption while holding a tracked spinlock → single-CPU priority-inversion deadlock — ROOT-CAUSED & FIXED 2026-07-01

**Where:** `kernel/src/sched/mod.rs` (`do_deferred_preempt`), `kernel/src/sync.rs`
(`Mutex::lock`/`try_lock`/`MutexGuard::drop`). Manifested as a hang in
`accounting::self_test` on the `ACCT` lock (`kernel/src/mm/accounting.rs`).

**This is the true root cause of the long-standing intermittent
spawn/kill/reap / accounting-self-test hang** previously filed as **F6**
("Accounting self-test hang — LIKELY CURED INCIDENTALLY", further below) and
related to the B-PTHREAD-YIELDBUDGET / TD31 "total silence, no dump"
fingerprint. F6 was never actually cured — it just didn't recur in the soak
because the trigger is timing-dependent (~5%). The spinlock stall detector
(commit `c8c1fa63`) finally caught it red-handed.

**Symptom / evidence:** boot hangs mid-`accounting` self-test. The stall
detector prints:
```
[sync] *** SPINLOCK STALL *** lock 'ACCT' ... (cpu 0, task 0, ... iters)
[lockdep]   cpu 0 holds 2 lock(s): [0] ACCT [1] ACCT
```
The "recursive" `[0] ACCT [1] ACCT` is NOT true recursion. lockdep's held
stack is **per-CPU** and is not cleared on context switch, so `[0]` is the
still-tracked entry of a task that was **preempted while holding `ACCT`**, and
`[1]` is a second, higher-priority task now spinning to acquire the same lock —
both accumulated on cpu 0's held stack.

**Root cause:** a kernel spinlock must never be held across a context switch.
`crate::sync::Mutex` did not disable preemption while held, so the timer ISR
could involuntarily preempt (`do_deferred_preempt` → `preempt`) a task
mid-critical-section. On a single CPU, if a higher-priority task (e.g. the
prio-31 boot self-test driver) then spins on that lock, the preempted holder
can never be rescheduled to release it → permanent deadlock. `do_deferred_preempt`
already had a *SCHED-only* guard (`SCHED.is_locked()`) for exactly this hazard —
it was a band-aid that covered one lock instead of the general invariant.

**Fix (the proper, general one):** a per-CPU preempt-disable count
(`PREEMPT_DISABLE_COUNT`, Linux `preempt_count` analogue). `Mutex::lock`/
`try_lock` call `sched::preempt_disable()` for the whole hold; `MutexGuard::drop`
calls `preempt_enable()` **after** the physical unlock (the inner spin guard is
now held in `ManuallyDrop` so the unlock is ordered before the enable — closing
the tiny window where a tick could switch away with the lock still physically
held). `do_deferred_preempt` refuses to involuntarily switch while
`preempt_count(cpu) > 0`, re-arming `NEED_RESCHED` so the preemption lands on a
later tick after the lock is released. Interrupts stay **enabled** (this is
preempt-disable, not IRQ-disable); locks also taken from a hardware ISR (e.g.
cgroup `TABLE` via `timer_tick`) already use `try_lock` on the ISR side, so
preempt-disable alone is sufficient.

**Verification:** 3× consecutive green boot tests (193–196s), accounting
self-test now passes the previously-deadlocking "Largest RSS" step; no
`SPINLOCK STALL` in the serial log; clippy clean on both changed files.

**Limitation / follow-up:** the guard covers *involuntary* preemption only.
Voluntarily yielding/blocking (`yield_now`/`block`) while holding a tracked
spinlock is still a caller bug and is not guarded (there is no such call site
today). **Done (2026-07-01):** added a one-shot warning in `schedule_inner`'s
voluntary-switch path when `preempt_count(cpu) > 0` (commit `49c92d346`);
it stayed silent across all boots, confirming no offending call site exists.
Also added (commit `ebd5c4b21`) a lockdep instant SELF-DEADLOCK diagnostic when
the *same* lock instance is re-acquired on one CPU — fires immediately instead
of waiting ~30s for the stall detector, now reliable because tracked mutexes no
longer carry stale per-CPU held-stack entries across a context switch.

**Raw `spin::Mutex` audit (2026-07-01):** the preempt-disable fix protects only
`crate::sync::Mutex`; a *raw* `spin::Mutex` (250+ call sites, mostly procfs/sysfs
leaf backends) held across a preemptible path and contended by a higher-priority
task is the same latent deadlock class — and is *invisible* to both lockdep and
the stall detector. Audited the only plausibly-dangerous category, the blocking
IPC primitives (`futex`, `pipe`, `stream_socket`, `semaphore`, `eventfd`,
`epoll`, `timerfd`, `signalfd`): **all clean** — every one follows the correct
enqueue-waiter → `drop(table)` → `block_current()` discipline (e.g.
`futex.rs:340-379` scopes the table lock in a block that closes before the park).
The remaining raw-`spin::Mutex` uses are short snapshot copies where the
held-across-preempt window is a handful of instructions and cross-priority
contention is implausible. **Proper systemic fix (deferred tech-debt):** migrate
kernel-internal raw `spin::Mutex` to `crate::sync::Mutex` so *all* kernel
spinlocks disable preemption and get lockdep coverage — gated on first checking
the lockdep class-table capacity (a 250-lock bulk migration could overflow it),
so it needs a capacity bump or a per-class opt-in rather than a blind sweep.

### B-ACCT-LARGEST. `accounting` self-test "Largest RSS" assumed test-only isolation, panicking when a live process held >50 RSS frames — FIXED 2026-06-30

**Where:** `kernel/src/mm/accounting.rs`, self-test "Largest RSS"
section (was ~line 507). The test charged two fake PML4s (a=20, b=50)
then asserted `largest_rss().pml4_phys == pml4_b`. But `largest_rss()`
scans the **global** accounting table, which during a live boot also
contains *real* process address spaces. Whenever a concurrent real
process happened to hold >50 frames at that instant, `largest` was that
real PML4 (e.g. `0x1DFE0000`, not the fake `0xBEEF0000`), so the
`assert_eq!` panicked and **hard-halted the whole boot**, masking every
self-test after it. A load-dependent flake: it passed on light boots
and failed under heavier ones.

**Fix:** the assertion was false-isolation; replaced with invariants
that hold deterministically even with real entries present:
(1) among the test's own entries, `query` confirms b (50) outranks
a (20); (2) `largest_rss().rss_frames >= 50` — i.e. it returns a true
global upper bound — instead of asserting it equals a specific fake
PML4. Verified: clean build + green boot self-test.

### B-CONTAINER-JAIL-TESTRACE. `container` self-tests 18/19 (rootfs jail + volume mounts) flaked non-deterministically: spawned a real init process, then inspected its per-PID namespace state, which the process cleared by exiting mid-test — FIXED 2026-06-30

**Where:** `kernel/src/container.rs`, self-tests "Rootfs jail (chroot) for
init process" (Test 18) and "Volume (bind) mounts for init process"
(Test 19). Both originally did `let pid = run(ct, HELLO_ELF, &opts)` to
spawn a *real, schedulable* init process, then called
`namespace::resolve_path_for(pid, …)` several times to assert the chroot/
volume wiring. The race: `HELLO_ELF` prints one line and **exits
immediately**; on another CPU it could run and exit *between* two of the
test's resolves. Thread teardown on exit calls `namespace::detach(pid)`,
which drops `PROCESS_ROOT[pid]`/`PROCESS_MOUNTS[pid]`, so a later
`resolve_path_for(pid, …)` returned the **unjailed input verbatim** and
the `assert_eq!` panicked → hard-halted the boot. Observed as Test 18's
`..`-escape assert failing on a heavy boot while an identical-binary
re-run passed (load-dependent flake). Production code is correct: a live
process resolves its *own* paths inside its own syscall handler, so the
jail always exists for the duration; only a third-party test reading
another process's namespace after it may have exited hits this.

**Fix:** Tests 18/19 no longer spawn a schedulable process. They register
a *synthetic, never-scheduled* PID through `add_process(ct, FAKE_PID)` —
the exact same container-layer wiring path `run()` uses
(`add_process_task` → `set_root`/`add_volume`) — and then run the
resolution asserts deterministically (the PID has no thread, so it cannot
exit and clear its state). The concerns that genuinely need a live
process are still covered without the race: the end-to-end
`run()`→cgroup-billing path by the "Run init process + cgroup billing"
test (Test 17), and the resolution *semantics* (`..` clamp, longest-
prefix volume match) by `namespace::test_process_root` /
`test_volume_mounts` (which already use synthetic PIDs 88888/88889). The
`state != Created` config-rejection guard is now exercised via `stop()`
rather than a live process, so it too is deterministic. Verified: clean
build + green boot self-test ("Self-test PASSED (19 tests)").

**Update (2026-06-30) — latent flake OBSERVED as a boot hang, now FIXED:**
The Test 17 liveness risk noted above stopped being theoretical. On a
heavy boot run the serial log froze mid-test right after the `run()` log
line (`[container] run id=8 'test-run-ct': init pid=219 …`) and never
reached `BOOT_OK` (480s timeout → boot gate FAILED). An identical-binary
re-run passed (`BOOT_OK after 187s`), confirming a load-dependent race,
not a logic bug — a timer ISR preempted the boot self-test thread into
the freshly-spawned init task, which executed `hello`; the exiting
thread's teardown then raced the test's explicit teardown, deadlocking
(a hang, not an assert panic — no `[PANIC]` was printed). This was worse
than the predicted assertion flake because a hang fails the *entire* boot
gate. **Fix:** Test 17's spawn→teardown window is now bracketed in
`cpu::without_interrupts(...)`, so the init task is still *registered*
(cgroup billing is verified end-to-end exactly as before) but can never
be *scheduled* before `destroy()` removes it — deterministic, with no
loss of real-`run()` coverage. Verified: clean build + green boot
self-test. Production code is unaffected (a live process only ever
resolves its *own* state inside its own syscall handler).

### B-ACCT-SPINLOCK-STALL. `ACCT` (mm memory-accounting) spinlock self-deadlock — ROOT-CAUSED + FIXED 2026-07-03

**STATUS: FIXED** (commit this session). Root cause confirmed by the
owner-tracking instrumentation: a **recursive self-deadlock** — the same task
that holds `ACCT` re-enters it from interrupt context. Fix: acquire `ACCT` via
the new `Mutex::lock_irqsave()` (interrupts masked for the hold), the standard
`spin_lock_irqsave` discipline for a lock shared with interrupt context. See
"Root cause + fix" below. Re-soak to confirm no recurrence.

**Root cause + fix (2026-07-03):** The instrumented soak reproduced on
**iteration 1** and the owner stamp printed the verdict verbatim:
`[sync]   lock 'ACCT' holder: task 138 == spinner — RECURSIVE self-deadlock
(same task re-entered the lock)` (task 138 = "countbytes", the ring-3
`/bin/emit | /bin/countbytes > file` pipeline; catch:
`build/hang-catches/ACCT-OWNER-recursive-task138.txt`).

Mechanism (uniprocessor — no cross-CPU AB-BA needed):
1. `Mutex::lock()` disables *preemption* but **not interrupts** — it leaves IF
   as-is. `ACCT` was acquired this way.
2. `ACCT` is reachable from **interrupt/softirq context**: the frame allocator
   calls `compact::try_compact()` for any `order > 0` allocation
   (`mm/frame.rs:2033`), and compaction's `estimate_movable_pages()` calls
   `accounting::tracked_count()` (`mm/compact.rs:266`) → acquires `ACCT`. So a
   device IRQ / softirq that allocates a multi-order buffer re-enters accounting.
3. Critically, the **page-fault handler re-enables interrupts** (`idt.rs:2048`,
   `cpu::sti()` when the faulting context had IF=1) *before* calling
   `mm::fault::resolve` → `map_frame`/CoW → `charge`/`uncharge`. So a
   `charge`/`uncharge` on the fault path runs and holds `ACCT` **with interrupts
   enabled**.
4. An interrupt lands while `ACCT` is held → its handler allocates an
   order>0 frame → compaction → `tracked_count()` → tries to re-acquire `ACCT`
   → spins forever (holder can never resume to release it). On UP the spinner
   *is* the same task's IRQ frame, so `owner == spinner` → the recursive verdict.

Why the earlier static analysis missed it: I looked only for a *direct*
IRQ-context accounting caller and found none; the real path is indirect
(IRQ → frame alloc → compaction → `tracked_count`) and is only opened by the
page-fault handler's `sti`. The accounting functions themselves remain correct
leaf scans; the bug was the *locking discipline*, not the functions.

**Fix:** added `Mutex::lock_irqsave()` + `MutexIrqGuard` to `kernel/src/sync.rs`
(save IF, `cli`, acquire; guard restores IF after releasing the lock and
re-enabling preemption — reverse of acquire order; nests correctly, only the
disabling edge restores). Switched all 12 `ACCOUNTING.lock()` sites in
`kernel/src/mm/accounting.rs` to `lock_irqsave()`. This masks interrupts for the
short leaf-only hold, closing the re-entry window for *any* interrupt (not just
the compaction path). A nested #PF cannot occur during the hold (the functions
only touch a static `.bss` array + trivial stack), so masking maskable
interrupts is both necessary and sufficient. Builds clean, no new clippy
warnings. Module doc in `accounting.rs` updated to document the IRQ-safety
requirement.

**Follow-up (separate, low priority):** `all_stats()` still `.collect()`s a
`Vec` under the lock (now under `lock_irqsave`, so interrupts are masked across
a heap alloc — worse for IRQ latency, though it has no live callers). Should be
count-then-release or a fixed stack buffer regardless.

---

<details><summary>Original investigation notes (pre-fix, kept for history)</summary>

#### B-ACCT-SPINLOCK-STALL. `ACCT` (mm memory-accounting) spinlock stuck at end of ring-3 battery — REPRODUCED 2026-07-03

**Where:** `kernel/src/mm/accounting.rs` (the `ACCOUNTING` spinlock, named `b"ACCT"`,
line 102) / `kernel/src/sync.rs` (the `Mutex` wrapper). Caught by the armed
hang-repro soak on **iteration 7/24** with the orphaned-Running-fixed kernel:
`build/hang-catches/ACCT-STALL-iter7-*.txt`.

**This is a DISTINCT bug from the orphaned-Running dispatch wedge** (which was
committed just before this soak). Decisive discriminator: the catch shows **no
`[sched] BUG:` line**, so the fixed dispatch path is not involved.

**Observed signature (`ACCT-STALL-iter7`):**
- `[liveness] SYSTEM HANG: no task-level forward progress for 15+ seconds
  (useful_work=140, all CPUs idle-ticking)` — cpu0 heartbeat=3501 **still
  advancing** (BSP alive, not an IF=0 spin), `local_has_real_work=false`,
  `last_rip=0xffffffff81107fb9 (kernel_text)`.
- Task dump: **91 tasks, 90 `state=Dead`, only `tid=0` (the boot/self-test
  driver, name overwritten to "prctl-batch269") is `state=Running`** on cpu0 at
  prio=31. This is the very end of the ~34-test ring-3 battery — everything ran
  and exited, leaving only the driver.
- Then: `[sync] *** SPINLOCK STALL *** lock 'ACCT' still not acquired after ~30s
  of spinning (cpu 0, task 0, 66805760 iters). Likely self-deadlock or lock
  convoy` followed by `[lockdep]   cpu 0 holds 0 lock(s):`. So task 0 spins
  ~66M iters trying to acquire `ACCT`, which the timer-driven liveness watchdog
  cannot rescue (the spin holds the CPU with preemption disabled).

**Analysis so far (static; not yet definitive):** The `ACCT` lock is
`mm/accounting.rs`'s `Mutex` (a `spin::Mutex` that does **not** disable
interrupts — `lock()` only `preempt_disable()`s). All *live* callers of the
accounting functions (`charge`/`uncharge` on the map/unmap/CoW page-fault path;
`query`/`tracked_count`/`largest_rss`/`memory_info` from procfs/kshell/
diagnostics/invariant checks) run in **task context** — I could not find any
IRQ/softirq/timer-context caller, which argues *against* a simple
interrupt-reentrancy self-deadlock. The accounting functions themselves are all
leaf scans that never yield/fault/allocate under the lock, so a single call
cannot leak the guard. The one structurally-unsafe function, `all_stats()`
(collects a `Vec` *under* the lock — violates the module's documented "ACCT is a
leaf lock, never held across other lock acquisitions" invariant), has **no live
callers**, so it is not the trigger here (but should be fixed on its own merits:
count-then-release or use a fixed stack buffer). `lockdep cpu 0 holds 0 locks`
is ambiguous — lockdep may only mark a lock *held* after successful acquire, so
a spinner shows 0, and the true holder (if since-dead) leaves no lockdep trace.

**Instrumentation added (commit this session; `sync.rs`) to make it definitive
on the next repro:** every `Mutex` now records the acquiring task id in a new
`owner: AtomicU64` (set in `make_guard`, cleared to `OWNER_NONE`=`u64::MAX` in
`MutexGuard::drop` — one relaxed per-CPU read+store, negligible next to the CAS
and lockdep call already present). `report_stall` now prints the holder and
classifies the stall:
- `owner == spinner tid` → **recursive self-deadlock** (same task re-entered).
- `owner == some other task` → **guard held by another task** (leaked if that
  task is Dead in the dump).
- `owner == OWNER_NONE` → **lost-unlock / flag desync** (spinlock flag set with
  no recorded holder).
This single datum discriminates all three hypotheses. Builds clean. **STILL OPEN
— re-run the armed soak with the instrumented kernel; the next `ACCT` stall will
name its holder and pin the exact leak/recursion path.**

</details>

### B-PAGECACHE-COHERENCE. Read-only page cache invalidation on FS mutations — FIXED 2026-06-30 (de-double-cache vs. buffer cache still pending)

**Resolution (2026-06-30):** the two correctness gaps below are now
closed. `mm::page_cache::invalidate_identity(fs_id, ino)` is wired into
the VFS mutation paths — `Vfs::write_at`, `Vfs::write_file`,
`Vfs::truncate`, `Vfs::remove`, and replacing same-mount `Vfs::rename`
— via the `cache_identity()` helper, which captures the file's
`(fs_id, ino)` under the held VFS lock (gated on a single relaxed
`is_populated()` atomic so the write path pays ~nothing when nothing is
cached). `remove` and the replacing-rename capture identity *before* the
inode is freed, closing the inode-reuse hole; the others capture after
the content change. Verified by boot self-test check 8 (is_populated +
invalidate_identity) and a green BOOT_OK.

**Shrinker (sub-task 4 eviction) landed 2026-06-30.**
`mm::page_cache::shrink(PressureLevel)` evicts *idle* cached pages
(refcount ≤ 1, i.e. no live mapper) proportional to the pressure level
(Low 25% / Medium 50% / Critical 90%), registered with `mm::pressure`
by `mm::page_cache::init()` (called from `kernel_main`). Verified by
boot self-test check 9 (shrink spares live, evicts idle) *and* by the
shrinker actually firing under real critical pressure during boot —
serial shows `[pressure] page_cache freed 49 objects (level=critical)`
then `freed 5 objects`, with BOOT_OK reached cleanly. Freeing 54 frames
under live pressure with no fault is a strong exercise of the
freed-while-mapped hypothesis: a mapped cache page always has
refcount ≥ 2 (cache entry + each PTE; `map_frame` does not bump
refcount, so the `get_or_fill` caller ref *becomes* the PTE ref), so
the shrinker's `refcount <= 1` gate never selects a mapped frame.

**Still pending (performance, not correctness — §36 sub-task 4 tail):**
de-double-cache the page cache against the block buffer cache
(`fs/cache.rs`) so a page does not live in both. Tracked as a follow-up;
not a bug.

The original write-up (now resolved for the correctness parts):



**Where:** `kernel/src/mm/page_cache.rs` (the cache) + the VFS/handle
write/truncate/unlink/rename paths (`kernel/src/fs/handle.rs`,
`kernel/src/fs/vfs.rs`, and the relevant syscall translators in
`kernel/src/syscall/linux.rs`).

**What it is:** sub-task 3 (commit wiring the FileBacked fault path to
`page_cache::get_or_fill`) populates the cache from mmap faults but does
**not** yet invalidate cached pages when the backing file changes. Two
correctness gaps result:

1. **Stale data after write/truncate.** If process A `mmap`s a file
   (pages enter the cache) and process B `write(2)`s or `ftruncate(2)`s
   that same file, A keeps seeing the *old* bytes through its mapping
   (and any later mmap of the file gets the cached stale page). The
   cache is read-only by design (writable MAP_SHARED writeback stays
   ENOSYS, §23), but read-side coherence with `write(2)` is still
   required and is missing.

2. **Inode-number reuse.** The cache key is `FileId { fs_id, ino }`.
   `fs_id` is monotonic per-mount (never reused), but `ino` **can** be
   reused within a mount after `unlink`. If file X (ino 53) is cached,
   unlinked, and a new file Y reuses ino 53, a fault on Y would be
   served X's stale pages. (`fs_id` prevents *cross-mount* collisions
   only.)

**Effect:** wrong file contents observed through a file mapping after a
concurrent write/truncate, or after unlink+recreate reuses an inode.
Not hit on the boot path (programs mmap read-only shared objects they
don't concurrently rewrite), which is why boot is green — but it is a
real correctness bug for general workloads.

**Proper fix (sub-task 4):** wire cache invalidation to FS mutations:
`page_cache::invalidate_file(file_id)` (or a page-range invalidate) on
`write`/`pwrite` that extends/overwrites a regular file, on `truncate`/
`ftruncate`, and on `unlink`/`rename` that drops/replaces an inode.
Resolve the `FileId` cheaply at the mutation site (the handle/path is
already known). Keep it cheap when nothing is cached (the
BTreeMap-range invalidate already returns 0 fast for an absent file).
Also de-double-cache against the block buffer cache (`fs/cache.rs`) per
§36 sub-task 4. Until this lands, the page cache is only safe for the
read-mostly mmap workloads the boot path exercises.

**Discovered/created:** 2026-06-30 (completing sub-task 3 without
sub-task 4's coherence wiring).

### B-CGROUP-DBLCHARGE. Demand-fault paths double-charge cgroup memory (manual `try_charge_current_mem` + `alloc_frame`'s internal charge) — FIXED (2026-06-30)

**Where:** `kernel/src/proc/pcb.rs` — `try_resolve_fault` demand-paging
paths. The whole-frame anon/file fast path (and the subpage path) call
`try_charge_current_mem(1)` *and then* `frame::alloc_frame()`, but
`alloc_frame` already charges the current task's cgroup internally
(`charge_cgroup_alloc`, recording the per-frame cgroup id in the
`FRAME_CGROUP` array). At final free, `free_frame` performs exactly one
`uncharge_cgroup_free` using the recorded id.

**Effect:** when cgroup memory accounting is active (`CGROUP_MEM_ACTIVE`
true), each demand page fault charges the cgroup **twice** (manual +1
and alloc_frame's +1) but uncharges only **once** at the final frame
free → a net **+1 charge leak per faulted page**. Over a process's
lifetime this inflates the cgroup's accounted memory without bound,
which can spuriously trip the cgroup memory limit / OOM. When cgroup
accounting is inactive (the common boot path), both charge calls
fast-exit, so there is no visible effect — which is why this has gone
unnoticed.

**Proper fix:** remove the manual `try_charge_current_mem(1)` /
`uncharge` bookkeeping from the demand-fault paths and rely solely on
`alloc_frame`/`free_frame`'s internal per-frame cgroup charging (which
is already correct and balances at the final free). The only subtlety:
`try_charge_current_mem` is also the place that enforces the *limit*
(returns an error to fail the fault when over budget) — so the fix must
ensure `alloc_frame` itself honors the cgroup limit (fail allocation
when the charge would exceed the limit) before deleting the manual
pre-check, otherwise the limit stops being enforced on the fault path.
Verify against the cgroup memory-limit self-test after the change.

**Discovered:** 2026-06-30 while wiring the page cache into the
FileBacked fault path (the cached-hit branch correctly needs *no*
manual charge, which surfaced the existing double-charge on the miss
branch).

**Fixed:** 2026-06-30. Removed the manual `try_charge_current_mem(1)` /
`uncharge_current_mem(1)` bookkeeping from both demand-fault paths in
`kernel/src/proc/pcb.rs` (subpage and whole-frame); the frame allocator
now owns cgroup memory accounting end-to-end. `alloc_frame` /
`alloc_frame_zeroed` already charge the current task's cgroup and honor
its limit (returning `Err(OutOfMemory)`, which the fault paths now
propagate as a rejected fault), so the deleted manual pre-check did not
weaken limit enforcement. Also closed two latent charge holes on the
zero-pool path: `alloc_frame_zeroed`'s pool-pop fast path now charges
the consumer, and `refill_zero_pool` uncharges frames it parks in the
pool (pooled frames are uncharged free inventory; the charge lands when
a consumer pops one). Regression guard: `mm::frame` self-tests 12
("charge/uncharge round-trip — no double-charge") and 13 ("over-limit
charge leaves no record"), which drive the real `charge_cgroup_alloc_to`
/ `uncharge_cgroup_free` primitives against an explicit test cgroup
(kmain self-tests run with no scheduled task, so the ambient
current-task cgroup is always root). Both pass in QEMU; the existing
cgroup charge/uncharge and limit-enforcement self-tests (10/11) still
pass.

### D-CGROUP-TASK-UNASSIGNED. Cgroup memory controller now reachable for real workloads — RESOLVED (2026-07-01)

**Original problem:** every `Task` was constructed with
`cgroup_id: ROOT_CGROUP` and no path ever set it to anything else, so
`current_task_cgroup()` always returned root, `charge_cgroup_alloc`
fast-exited, and the per-cgroup memory limit / accounting was never
exercised by real workloads — only by self-tests charging an explicit
cgroup. Container memory limits did not actually constrain memory.

**Resolution (Q14, operator option A):**
1. **Assignment path** — `sched::set_task_cgroup(task_id, cgroup)`
   (`kernel/src/sched/mod.rs:1287`) is the single authoritative
   process→cgroup assignment: it swaps `task.cgroup_id` under the SCHED
   lock and keeps the cgroup `nr_tasks` counts consistent (detach old,
   attach new) with a strict SCHED→cgroup-TABLE lock order.
   `container.rs` `add_process_task` (line ~1543) calls it to move a
   container's task into the container's cgroup, and `remove` (line
   ~1640) moves it back to root.
2. **Inheritance path** — `sched::spawn` (`mod.rs:1031/1046`) captures
   `current_task_cgroup()` before the task-creation critical section and
   copies it onto the new task, so `fork` (routes through
   `thread::spawn`→`sched::spawn`), `thread_clone`, and `spawn_user`
   (also `→sched::spawn`) all inherit the creating task's cgroup — Linux
   fork/clone semantics. Recorded in design-decisions §39.
3. **End-to-end test** — `cgroup_e2e_test_task` in `kernel/src/main.rs`
   runs as a live scheduler task (so `current_task_cgroup()` resolves to
   a real task, unlike the no-task kmain self-tests): it creates a
   memory-limited child cgroup, joins it via `set_task_cgroup`, allocates
   N=32 frames through the ordinary `alloc_frame` path (into a stack
   array — no heap growth to perturb the count), and asserts the group's
   `mem_usage` rose by exactly N; then frees them and asserts usage
   returns to baseline (uncharge follows the per-frame `FRAME_CGROUP`
   record, so it debits the right group even after the task rejoins root).
   Prints `[cgroup-e2e] PASS`/`FAIL` on the boot serial log.

**Discovered:** 2026-06-30 while fixing B-CGROUP-DBLCHARGE. **Resolved:**
2026-07-01 once Q14 settled which layer owns process→cgroup assignment
(`kernel/src/cgroup.rs` enforces + owns assignment via `set_task_cgroup`;
`fs::cgroupfs` remains the config frontend).

### D-CONTAINER-EXEC-WAIT. Real in-container `docker exec` + synchronous wait — RESOLVED (all four steps landed)

**Status (2026-07-01): steps 1–4 done and boot-validated.** `container
exec` is no longer a net_ns-switch facade — it launches a genuine process
inside the container and (foreground) blocks until it exits, printing the
exit status. Step 4 (healthchecks) now landed too: the OCI `Healthcheck`
config is parsed, stored on the container, and driven by a periodic
non-blocking supervisor that surfaces health in `inspect`/`ps`.

**What landed:**
1. `container::wait_process(pid) -> KernelResult<i32>`
   (`kernel/src/container.rs`): the generalised block-on-exit primitive.
   Parks the caller on an arbitrary spawned global pid via
   `pcb::set_wait_task` + `sched::block_current`, woken by the
   zombie-transition path (`remove_thread` hands back the registered
   wait-task). Lost-wakeup-safe (re-check after register + scheduler
   `pending_wake`). On zombie it reads `pcb::exit_code(pid)` and reaps via
   `pcb::try_reap`, so an exec'd non-init child never lingers unreaped.
2. `container::exec_path(id, guest_cmd, argv) -> KernelResult<ExecSpawn>`:
   resolves `guest_cmd` under the container rootfs (`resolve_in_rootfs`,
   `..` cannot escape), reads the ELF, `spawn_process`es it, and
   `add_process_task`s it into the container's cgroup + PID/user/network
   namespaces + rootfs jail (the `run` wiring, minus flipping state /
   recording `init_pid`). Rolls the spawn back on bind failure. Stdio is
   left at the console default (foreground output appears live).
3. Shell `container exec [-d] <id> <cmd> [args...]`
   (`kernel/src/kshell.rs`, cmd_container "exec" arm): builds argv from the
   tokens, calls `exec_path`; foreground → `wait_process` + print exit
   status + `remove_process_task` cleanup; `-d` → print pid and return.

**Root-cause fix bundled in:** cgroup task-count accounting was previously
decremented **only** by an explicit `set_task_cgroup`/`remove_process_task`
while the task was still alive; a task that simply *exited* while assigned
to a non-root cgroup left a stale `nr_tasks` count forever (the task is
gone from the scheduler table before anyone can move it back to root).
`sched::reap_dead_tasks` now auto-detaches a reaped task from its cgroup
(skipping the root group; `detach_task` is saturating so a
detach-then-die can't underflow). This makes teardown accounting robust
for *any* exiting task, not just exec'd ones.

**Validation:** boot self-test `[container]   exec + wait
(exec_path/wait_process): OK` — creates a Running container with a real
rootfs, stages `/bin/hello`, execs it, yields until it zombifies, and
asserts: exit code 0 captured, process reaped (`pcb::state` is `None`),
cgroup billed +1 while alive then 0 after reap, plus the error paths
(exec on a non-Running container → InvalidArgument, missing binary →
NotFound, `wait_process(bogus)` → NoSuchProcess). BOOT_OK, hello's stdout
observed once in the serial log.

**Step 4 (healthchecks) — landed:** `oci::HealthcheckConfig`
(`kernel/src/oci.rs`) parses the OCI `Healthcheck` (test-token +
interval/timeout/retries/start_period, CMD vs CMD-SHELL). Each container
stores the probe plus its live health state
(`health_status`/`health_fail_streak`/`health_started_ns` and the
in-flight probe pid/task/deadline). The pure state machine
`container::apply_probe_result` implements the Docker semantics
(start-period grace does not count failures while `Starting`; a
`retries`-long failure streak → `Unhealthy`; any pass → `Healthy` + reset
streak) and is unit-covered by boot self-test `19k2h`.

The probes are driven by a **non-blocking** supervisor: a persistent
repeating `hrtimer` (250 ms tick, `start_health_monitor`, armed just
before `BOOT_OK` so it can't perturb the hrtimer self-test's exact
`pending_count` assertion) fires in ISR context, submits `health_tick_job`
to the shared `workqueue`, and `health_tick` polls every container.
Critically it **never blocks the single workqueue worker**: each probe is
launched via `exec_path`, then *polled* for its zombie transition on
subsequent ticks (never `wait_process`-blocked), reaped via the
`wait_process` fast path once dead, scored via `apply_probe_result`, and a
probe that overruns its timeout is `kill_process_threads`'d and scored as
a failure. The tick uses snapshot-under-lock → act-outside-lock (exec /
reap / kill / remove all take the table lock internally) → write-back.
Health is surfaced in `inspect` (JSON `health` field + human Health line
with failing streak) and `ps` (a `(healthy)`/`(unhealthy)`/`(health:
starting)` sub-state on the status column). Boot self-test `19k2s` drives
a real `/bin/hello` CMD probe deterministically to `Healthy`.

**Discovered/documented:** 2026-07-01 (while surveying the next container
increment after `docker network`). All four steps landed same day.

### B-COMPACT1. Memory-compaction self-test (`collect_private_frames`) panicked non-deterministically across boots — FIXED 2026-06-16

**Where:** `kernel/src/mm/compact.rs` — `self_test()` Test 5; the API under test is
`kernel/src/mm/rmap.rs::collect_private_frames`.

**What it was:** the self-test added one fake private rmap entry, then called
`collect_private_frames(&mut [0u64; 4], 0)` once and asserted the fake frame was
among the (up to 4) results. `collect_private_frames` fills its `out` buffer with
the first `out.len()` private frames in table-index order, starting from the cursor
and wrapping once around the whole 16384-slot table. By the time the compaction
self-test runs, the rmap table already holds entries from other subsystems (a
failing boot showed ~16). With a 4-slot buffer, only the four lowest-indexed
private frames are returned; whether the fake entry (hashed to slot
`0x0F00_0000 % 16384`) is among them depends on what else occupies lower slots —
so the assertion passed or panicked depending on unrelated boot state. The panic
(`"collect_private_frames should find our fake entry"`) aborted the kernel mid-boot,
failing the Path-Z boot-test.

**Fix (2026-06-16):** the test now pages through the table with a 32-slot buffer
(larger than the live entry count, so a single full sweep already finds every
private frame including the fake one) and a bounded loop that advances the
continuation cursor each page, breaking as soon as the fake frame is seen, the
table is exhausted (`found == 0`), the cursor stops advancing, or a 64-page hard
cap is hit (guaranteed termination). This makes the test deterministic regardless
of how many unrelated rmap entries exist. Verified: BOOT_OK with
`[compact]   collect_private_frames: OK (saw_fake=true)` and
`[compact] Self-test PASSED`, 0 self-test failures.

**Related debt (not fixed):** `collect_private_frames`'s continuation/pagination
is mildly broken as a "visit every unique private frame exactly once" iterator —
each call performs a *full* `0..RMAP_TABLE_SIZE` sweep from `start_idx`, so when
more than `out.len()` private frames exist the continuation re-encounters frames
below the cursor on the next page (it never returns the `(found, 0)` "scan
complete" sentinel). The production consumer
(`compact.rs::try_compact`, 4 batches × 32) tolerates this — it re-checks each
candidate via `try_migrate_one` and only wastes a little work re-examining
duplicates — so it is a performance/clarity wart, not a correctness bug. A proper
fix would have the continuation scan only the *remaining* `[next, original_start)`
window rather than re-sweeping the whole table. Tracked here; low priority.

### B-EXT4-DIR. ext4 directory entries past the first block became invisible, and every directory insert grew the directory by a full block — FIXED 2026-06-16

**Symptom:** The ring-3 `link()`/`linkat()` hard-link self-test
(`self_test_linux_link`, kernel/src/proc/spawn.rs) intermittently failed
with exit 193 (link failed). Tracing showed `Vfs::write_file("/mnt/lnk-src",
b"L")` returned `Ok` but the file was then unresolvable, and later
`link()` reported `AlreadyExists` for a name the VFS layer's `exists()`
could not see. The persistent `/mnt` ext4 fixture (rootfs.ext4) also grew
without bound across boots as the self-tests created and deleted files.

**Root cause (two independent ext4 directory bugs):**

1. **`parse_dir_entries` abandoned the whole directory at the first
   `rec_len == 0`** (kernel/src/fs/ext4/driver.rs). ext4 directory data is
   a sequence of independent `block_size` chunks; a chunk can legitimately
   end with zero-padding (rec_len 0) while *later* blocks still hold live
   entries. The old loop `if hdr.rec_len == 0 { break; }` broke out of the
   entire directory, so every entry living in a block after the first
   zero-padded block was invisible to `read_dir_entries` → `dir_lookup` →
   path resolution. A file whose dirent landed in a later block "didn't
   exist" to `Vfs::exists`/`open`, yet `add_dir_entry`'s own physical scan
   still saw it (→ spurious `AlreadyExists`). It also meant `remove` could
   not find/unlink such entries, so they accumulated as orphans.

2. **`add_dir_entry`'s in-place-reuse path was dead code** (off-by-one).
   It computed the last directory block as `(dir_len / block_size) *
   block_size`, which for a block-aligned directory equals `dir_len`
   itself, so the guard `last_block_start < dir_len` was never true. Every
   insert fell through to the grow path, appending a fresh block per entry:
   unbounded directory bloat and fragmentation, which in turn fed bug (1)
   (more blocks → more chances for an entry to hide past a zero-padded
   block).

**Fix (proper):**

- Rewrote `parse_dir_entries` to parse block-by-block: an outer loop over
  `block_size` chunks and an inner loop over entries within
  `[block_start, block_end)`. `rec_len == 0` now terminates only the
  *current* block and advances to the next, never the whole directory.
  Name bounds use `block_end`, not `data.len()`. Added a regression test
  with a two-block buffer where block 0 ends in a zero-padded entry and
  block 1 holds a live entry, asserting both entries are found.
- Fixed `add_dir_entry` to compute the real last-block start as
  `dir_len.saturating_sub(block_size)` (guarded by `dir_len > 0 &&
  block_size > 0`), so free space in the final block is actually reused
  instead of growing the directory every time.
- Refactored `insert_dir_entry` to take an explicit `block_start`
  parameter (removing a buggy `(offset / remaining).max(1) * ...`
  reconstruction) and scan forward from it to find the previous entry to
  shrink.

**Verified:** With the fixes plus a freshly regenerated rootfs.ext4
(`wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh`), the ring-3
link()/linkat() self-test passes and the full boot reaches BOOT_OK with
zero self-test failures.

**Fixture note:** The pre-existing rootfs.ext4 had accumulated duplicate /
orphaned `lnk-dst` directory entries from prior buggy boots that the fixed
code could now see but a single `remove()` could not fully clear. The
fixture was regenerated clean. `self_test_linux_link` also gained a bounded
`drain()` loop that removes any stale src/dst names before staging, so the
test is robust to a dirty persistent fixture going forward.

### B-CWD1. Linux-ABI relative path resolution ignored the per-process cwd (relative `open`/`*at` resolved against `/`) — FIXED 2026-06-16

**Symptom:** After a process did `chdir("/dir")`, a relative `open("file")`
(or `openat(AT_FDCWD, "file")`, and the relative-path branches of stat,
access, mkdir, unlink, rename, readlink, chmod, chown, etc.) resolved the
path against the filesystem **root** rather than `/dir`. e.g. `cd /reltest &&
echo x > rel.txt` created `/rel.txt`, not `/reltest/rel.txt`. This broke
standard Unix semantics for essentially every program that uses relative
paths after changing directory.

**Root cause:** The Linux ABI's `open_common` forwarded the raw userspace
path pointer straight to `sys_fs_open` → `fs::handle::open` →
`Vfs::resolve_path`, and `resolve_at_path` (the `*at` family helper) returned
the path verbatim for the `AT_FDCWD`/relative case. None of those layers
take a PID, and `Vfs::normalize_path` treats `rel` identically to `/rel`
(it splits on `/` and always re-emits a leading slash), so the per-process
cwd stored in the PCB by `chdir` (`pcb::set_cwd`) was never consulted on the
open side. The limitation was even documented in `resolve_at_path`'s doc
comment ("there is no per-process cwd in the native path resolver").

**Fix (proper):** Resolve relative paths against the caller's cwd at the
Linux ABI boundary, reusing the existing `canonicalize_path(cwd, path)`
helper (already used by the chroot gate and `fstatat`). `open_common`
(kernel/src/syscall/linux.rs) now canonicalises the path against
`pcb::get_cwd(caller)` and opens via a new `handlers::fs_open_kernel_path`
(a kernel-string variant of `sys_fs_open` that does the File-READ cap check
+ handle registration without reading userspace), and `resolve_at_path`
canonicalises its `AT_FDCWD`/absolute result the same way. Kernel context
(no caller PID) falls back to cwd `"/"`, preserving the prior behaviour for
in-kernel callers and the native ABI (`sys_fs_open` is untouched). Absolute
paths are normalised but otherwise unchanged. Regression test: Path Z
Part 23 (`self_test_linux_real_glibc_shell_relpath`) runs `cd /reltest &&
echo RELOK > relfile.txt` in ring 3 and asserts the file landed at
`/reltest/relfile.txt` and **not** at `/relfile.txt`.

### B-ACCESS1. Linux-ABI `access`/`faccessat`/`faccessat2` always returned ENOENT (no-file skeleton-FS stub) — FIXED 2026-06-16

**Symptom:** Every `access`/`faccessat`/`faccessat2` call returned `-ENOENT`
unconditionally, even for files that exist in the VFS. The headline casualty
was unmodified GNU `make`: make issues `access("/bin/sh", X_OK)` **before**
spawning a recipe and, on failure, prints `"/bin/sh: No such file or directory"`
+ `Error 127` and never spawns the recipe shell — so no Makefile recipe could
run. (Confirmed via `strace` on real Linux: `access(shell, X_OK) = 0` precedes
the `clone3`.) Same class of stale stub as B-STAT1, but for the accessibility
probes rather than `stat`.

**Root cause:** `sys_access` / `sys_faccessat` / `sys_faccessat2` validated the
mode/flag bits and the path pointer, then hard-coded `linux_err(errno::ENOENT)`
with a comment that "without a backing filesystem there is no path that exists."
True when written; a silent lie once the VFS gained a backing store.

**Fix (proper):** The three syscalls now share a new `access_path_common`
back-end (kernel/src/syscall/linux.rs) that canonicalises the path against the
caller's cwd (`pcb::get_cwd`) and looks it up via `Vfs::metadata` (follow) /
`Vfs::lmetadata` (`AT_SYMLINK_NOFOLLOW`). Under the no-DAC capability model
(design-decisions §31) `F_OK`/`R_OK`/`X_OK` succeed for any existing file/dir —
consistent with `execve`, which ignores on-disk x-bits. Kernel context (no
caller PID) preserves the ENOENT no-file contract the fidelity self-tests
assert. Regression test: Path Z Part 34 (`self_test_linux_real_glibc_make`) runs
real GNU make end-to-end, whose recipe dispatch depends on `access(shell, X_OK)`.

**Known limitation (W_OK):** `W_OK` is granted for any existing file; it does
not yet consult per-mount read-only state (not tracked at this layer). A
read-only mount should return `EROFS` for `W_OK`. Low priority — no read-only
mounts are exposed to ring-3 writers today.

### B-FALLOC1. Linux-ABI `fallocate` COLLAPSE_RANGE/INSERT_RANGE now shift contents; only UNSHARE_RANGE still EOPNOTSUPP — PARTIALLY RESOLVED 2026-06-18, COLLAPSE/INSERT ADDED 2026-06-20

**Status:** `sys_fallocate` (syscall #285) was wired 2026-06-16 (Path Z Part 33)
from a blanket EOPNOTSUPP terminal to the real VFS for the two *allocate* modes:
`mode == 0` (posix_fallocate grow → `Vfs::file_size`/`Vfs::truncate`, never
shrinking) and `FALLOC_FL_KEEP_SIZE` (block reservation → `Vfs::fallocate`).
MemFd fds grow via `ipc::memfd::truncate`. Both enforce `RLIMIT_FSIZE` (EFBIG)
and the File-WRITE capability.

**Update 2026-06-18 — PUNCH_HOLE / ZERO_RANGE implemented.** The two most
commonly used range modes now do real work instead of returning EOPNOTSUPP. New
helpers `fallocate_zero_vfs` / `fallocate_zero_memfd` (kernel/src/syscall/linux.rs)
zero `[offset, offset+len)` in 16 KiB chunks via the backend's efficient
`write_at` (ext4/fat/memfs all override it). `i_size` is preserved for PUNCH_HOLE
(always KEEP_SIZE) and ZERO_RANGE+KEEP_SIZE — the zeroed region is clamped to the
current size and a range entirely past EOF is a no-op; ZERO_RANGE *without*
KEEP_SIZE grows the file to `offset+len` if the range crosses EOF, zero-filling
the gap. This is correct **read-as-zero** behaviour; the only thing not provided
vs. a real hole-punch is **disk-space reclamation** (an optimisation, not a
correctness property — our backends are non-sparse). Covered by
`self_test_fallocate_range` (registered in kernel/src/main.rs as a late, post-/tmp
self-test) which exercises ZERO_RANGE+KEEP_SIZE, PUNCH_HOLE, a past-EOF KEEP_SIZE
no-op, a ZERO_RANGE grow, and a MemFd ZERO_RANGE — all green at boot.

**Update 2026-06-20 — COLLAPSE_RANGE / INSERT_RANGE implemented.** Both
content-shifting modes now do real work for regular files (`HandleKind::File`)
instead of returning EOPNOTSUPP. The dispatch (kernel/src/syscall/linux.rs
`sys_fallocate`) enforces the full Linux contract: it queries the backing fs
block size via `Vfs::statvfs` and rejects a non-block-aligned `offset`/`len`
with EINVAL; COLLAPSE at/past EOF is EINVAL (Linux says use ftruncate); INSERT
at/past EOF (`offset >= size`) is EINVAL; INSERT also re-checks RLIMIT_FSIZE
against the *grown* size (`size + len`). The shifts themselves are chunked
(16 KiB) memmoves over `Vfs::read_at`/`write_at`: `fallocate_collapse_vfs`
slides the tail down (ascending copy, dst < src) then truncates by `len`;
`fallocate_insert_vfs` grows the file, slides the tail up (descending copy to
avoid clobber) then zeroes the inserted `[offset, offset+len)` hole. Our
backends are non-sparse, so this is a true content collapse/insert (not an
extent splice) — byte-for-byte identical from a reader's view; the only thing
not provided vs. a native ext4 extent op is the in-place efficiency, an
optimisation, not a correctness property. Covered by `self_test_fallocate_range`
cases (6)-(8): COLLAPSE_RANGE, INSERT_RANGE, and an INSERT+COLLAPSE round-trip
identity, all green at boot. A backend whose `statvfs` reports `block_size == 0`
(can't validate the alignment contract) keeps the EOPNOTSUPP fallback.

**Remaining limitation:** `UNSHARE_RANGE` still returns EOPNOTSUPP — it is a
reflink/CoW unshare concept our backends don't implement (there are no shared
extents to unshare). Well-behaved callers treat EOPNOTSUPP as "operation
unsupported" and skip it or fall back, so nothing breaks.

**Proper fix (deferred) for UNSHARE:** once a backend grows reflink/CoW extents
(none do today), dispatch UNSHARE_RANGE to a preallocate-and-unshare path; on a
non-reflink fs it is correctly a no-op (nothing is shared), so the EOPNOTSUPP
terminal is the conservative choice until reflinks exist. Kernel context
(caller_pid None) keeps the EOPNOTSUPP terminal for every mode, asserted by the
batch-536 FMODE_WRITE + vfs_fallocate gate-order self-tests.

### B-SIG1. dash's `wait` builtin (background-job reap) livelocked: no SIGCHLD on child exit + `rt_sigsuspend` was a stub — FIXED 2026-06-16

**RESOLVED 2026-06-16.** A real glibc `dash` running `/bin/emit > file &
wait` (Path-Z self-test `self_test_linux_real_glibc_shell_bgjob`) hung the
boot thread to a timeout. dash's `wait` builtin uses
`dowait(DOWAIT_BLOCK|DOWAIT_WAITCMD)`, whose `waitproc` computes
`flags = WNOHANG` (because `DOWAIT_WAITCMD` makes `block != DOWAIT_BLOCK`),
then loops `while (!gotsigchld && !pending_sig) sigsuspend(&oldmask)` —
relying on SIGCHLD delivery (its handler sets `gotsigchld`). The
synchronous pipe/loop/cmdsub waits use blocking `waitpid` (flags 0) and
never needed SIGCHLD, which is why those parts passed.

Two kernel gaps caused the livelock, both fixed properly:

1. **SIGCHLD was never posted to the parent on child exit.**
   `kernel/src/proc/thread.rs::on_thread_exit` now posts SIGCHLD to the
   parent when a child becomes a zombie — via the Linux-ABI disposition
   path (`signal::set_pending_info`, delivered by
   `deliver_linux_signal` → `linux_disposition`) for Linux parents, and
   `classify_post_info` for native parents (SIGCHLD's default action is
   ignore, so a no-handler parent correctly drops it). This is distinct
   from the existing `wait4()` waiter wakeups, which target a thread parked
   in `wait4()`, not the signal path.

2. **`sys_rt_sigsuspend` was a stub returning EINTR immediately.** This
   made dash busy-spin (`sigsuspend` → EINTR → re-loop → …), starving the
   boot thread. It is now a real park loop modeled on `sys_pause`
   (`kernel/src/syscall/linux.rs`): it installs the temporary mask, parks
   on the signalfd wait-queue until a signal deliverable under that mask
   arrives, and restores the original mask correctly via a Linux
   `saved_sigmask`/`TIF_RESTORE_SIGMASK` mechanism — `emit_linux_rt_frame`
   writes the saved pre-suspend mask into the handler frame's `uc_sigmask`
   (so `rt_sigreturn` restores it), and the no-handler tail of
   `deliver_linux_signal` restores it directly. The contextless
   (in-kernel, `caller_pid()==None`) case still returns EINTR immediately
   so the existing rt_sigsuspend self-test is unaffected.

**Verify:** boot test reaches `BOOT_OK`; the bgjob self-test logs "read
back 16 bytes == expected, exit 0: OK".

### B-HEAP1. Kernel heap redzone "overflow" reports during init file-install were FALSE POSITIVES from a pre-poison allocation window — FIXED 2026-06-16

**Symptom (as originally observed):** During boot (init step 24, after all
self-tests), the debug heap allocator's dealloc-time redzone scanner reported
several `[heap] BUFFER OVERFLOW detected! slot=…, alloc=N, class=C, offset=N`
lines, e.g. `alloc=10, class=16, offset=10` (right before
`[init] Installed /bin/hello`) and two `alloc=18, class=32, offset=18`. Boot
still reached `BOOT_OK` and all self-tests passed.

**Root cause (NOT a real overflow):** The redzone check relies on the invariant
"every byte in `[alloc_size, class_size)` is `ALLOC_POISON` (0xCD)". That holds
only if the slot was `poison_alloc`'d *at the time it was handed out*. But
`enable_poison()` was called very late in boot (`kernel/src/main.rs` step 22f-3,
old line ~3518) while the heap is initialized far earlier (`mm::heap::init`,
~line 455). **Every allocation made in that window was never poison-filled.**
When such a slot was later freed *after* poisoning came online, `check_redzone`
scanned whatever bytes the pre-poison occupant had left there — zeroed
fresh-frame bytes, or stale content from an earlier reuse — and reported them as
overflow. Captured byte dumps confirmed this: a slot freed with `alloc_size=18`
held the intact 31-char string `/tmp/tmpwatch_test/delete_me.tmp` filling the
whole 32-byte class (a former occupant), and `"/bin/hello"+'e'+zeros` showed
unpoisoned (zero) redzone bytes — neither is possible if the slot had actually
been alloc-poisoned. So the reports were detector false positives, not memory
corruption.

**Fix:** Move `mm::heap::enable_poison()` to immediately after `mm::heap::init()`
(`kernel/src/main.rs`, step 6), *before the first heap allocation*. With no
pre-poison allocation window, every slab slot is poison-filled at its first
alloc and the redzone invariant always holds. The redundant late
`enable_poison()` at step 22f-3 was removed (the `poison_self_test()` call
stays). Poison is still toggled OFF only for the duration of the heap
benchmarks (`deferred_bench_task`), which free their own allocations within that
window. Note this only affects slab classes (≤ 8192 B); large allocations (the
actual MB-sized binaries) go through the buddy path and are never poisoned or
redzone-checked, so the early-enable adds negligible boot cost.

### B-DP1. `validate_user_range` rejected committed-but-not-yet-faulted-in demand-paged user buffers (EFAULT on large fresh output buffers) — FIXED 2026-06-16

**RESOLVED 2026-06-16.** `kernel/src/mm/user.rs::validate_user_range`
(the core of `validate_user_read`/`validate_user_write`) walked every
4 KiB page of a user buffer and returned `InvalidAddress` the moment
`page_table::translate()` reported a page *not present*. That is wrong
for **demand-paged** memory: a freshly-`malloc`/`mmap`'d buffer is
committed (covered by a VMA) but its pages are not populated until first
touched. A syscall handed such a buffer as an *output* target would
EFAULT on every page past the first, because the process had not yet
written to those pages itself.

**Reproduce:** run `dash -c 'echo /globdir/* > out'` (Path-Z real-glibc
self-test `self_test_linux_real_glibc_shell_glob`). glibc's `opendir`
allocates a 32 KiB dirent buffer and calls `getdents64` into it before
touching it; the buffer's later pages were not present, so
`validate_user_write(dirp, 32768)` returned EFAULT, `readdir` returned
NULL, and dash's glob matched nothing — emitting the literal `/globdir/*`
instead of the three filenames. (The directory open, VFS readdir, and
getdents64 encoding were all proven correct via tracing; the validation
pre-walk was the sole culprit.)

**Fix:** when the pre-walk finds a not-present page, call the new
`try_fault_in_user_page(addr, need_writable)`, which synthesizes an x86
page-fault error code (not-present + user + write-iff-needed) and routes
it through `crate::proc::pcb::try_resolve_fault` — the same demand-paging
resolver the hardware #PF handler uses — then re-checks `translate()`.
This mirrors Linux's `get_user_pages()` faulting pages in before a
kernel-side access. A genuinely unmapped or permission-violating address
still fails (the resolver returns `false`), so invalid pointers are still
rejected. **Validated:** the dash glob self-test now reads back the
expected 45 bytes (`/globdir/a.txt /globdir/b.txt /globdir/c.txt\n`),
exit 0; full boot test passes with no self-test failures.

### B-DF1. Kernel-stack overflow → double fault when an IRQ frame pushes onto a near-full kernel task stack (deferred benchmark suite) — FIXED 2026-06-15 (Q7 option A)

**RESOLVED 2026-06-15.** Fixed via `open-questions.md` Q7 → **option A**
(operator-chosen): a dedicated per-CPU guard-page IRQ stack with a manual
nesting-aware switch in `idt::irq_common_dispatch` (so hardware IRQ frames/
handlers never consume the interrupted task's stack), plus **deferred
preemption** (timer ISR sets `NEED_RESCHED`; the outermost IRQ frame runs the
context switch on the task stack via `sched::do_deferred_preempt`). The
restructuring also exposed an **unbounded re-entrant preemption recursion**
(nested timer tick during `schedule_inner`, with interrupts enabled on the task
stack, misclassified as a fresh outermost IRQ → recursion until guard-page
overflow); fixed by disabling interrupts across the involuntary switch in
`do_deferred_preempt`. See `design-decisions.md` §26. **Validated:**
`http_gzip_8KiB` — which previously double-faulted entering the dashboard benches
on a near-full task stack — now runs to completion.

**Follow-up 2026-06-15 — `BENCH_OK` now reached end-to-end.** After the Q7
landing, two further blockers were chased to ground:

1. **The previously-documented `bench_isr_latency` null-pointer crash no longer
   reproduces.** It was an artifact of the *old* timer-ISR path that called
   `preempt()` inline during the hard-IRQ handler; the Q7 deferred-preempt
   restructuring (timer ISR only sets `NEED_RESCHED`; the switch runs later on
   the task stack) removed it. Verified by running `bench_isr_latency()` both
   early and in its normal end-of-suite slot — it completes cleanly (≈54 µs
   hard-IRQ phase under TCG, above the 10 µs target but that is emulation
   noise, not a fault). The stale `todo.txt` "Cross-Zone Bug Reports" entry is
   superseded.

2. **The actual last `BENCH_OK` blocker was a scheduler self-deadlock, now
   fixed.** `bench_dashboard_api_status` calls `dashboard::api_status()` →
   `sched::task_list()`, which holds `SCHED` (a plain `spin::Mutex`) across a
   heap `Vec` collect over *all* tasks. Run 1000× in a tight loop, a timer tick
   reliably lands while the task holds `SCHED`; the Q7 deferred-preempt then ran
   `preempt() → schedule_inner() → SCHED.lock()` on the *same* CPU and spun
   forever (the `cli` in `do_deferred_preempt` made the hang unrecoverable). The
   fix: `do_deferred_preempt` now checks `SCHED.is_locked()` and, if held,
   re-arms `NEED_RESCHED` and defers to the next tick instead of blocking — the
   same try/skip discipline `unthrottle_expired()` already uses from ISR
   context. This closes the *entire* "involuntary preempt while the interrupted
   context holds SCHED" deadlock class (including the tiny analogous window
   during voluntary `yield_now`/`block`), at the single involuntary-preempt
   site. **Validated: the full `--bench` suite now reaches `BENCH_OK` ("Boot
   test PASSED").** See `design-decisions.md` §27.

The original analysis is retained below for history.

**Root cause (CONFIRMED): kernel task stack overflow into the guard page.**
The deferred benchmark suite runs heavy, *debug-built* code paths in kernel
context (gzip/deflate, `format!`-heavy JSON, crypto) on a kernel task with a
fixed **64 KiB** stack (`TASK_STACK_SIZE = 4 * 16 KiB`). The kstack allocator
(`kernel/src/mm/kstack.rs`) lays out each task stack as `[guard 16 KiB][stack
64 KiB]`, slot stride `SLOT_SIZE = 0x14000`, region base `0xFFFF_C100_0000_0000`.
The reported fault `RSP = 0xffffc1000003ffb8` decodes to slot 3, within-slot
offset `0x3FB8`, which is **< GUARD_SIZE (0x4000)** — i.e. RSP is **inside the
guard page**, ~72 bytes below `stack_bottom`. So the stack overflowed; the
faulting `atomic_load` (and the IRQ frame that the CPU was pushing) landed on
the unmapped guard page → the fault could not be delivered → #DF.
(Correction to an earlier note: RSP is **not** "near the top of the stack" — I
had mis-decoded the slot stride. It is firmly in the guard page. The two
backtrace frames are the #DF handler's own IST stack — `handle_double_fault` /
`isr_double_fault` — and are uninformative.)

**Why an IRQ tips it over.** Hardware IRQs (timer vector 32; device IRQs 33–56,
incl. mouse IRQ12) are installed in the IDT with **IST index 0** (see
`idt.rs::init`, `IdtEntry::new(..., 0, 0)`) — they run on the *current* kernel
task stack, not a dedicated stack. When a benchmark has driven the task stack
near `stack_bottom`, the CPU pushing the interrupt frame (and the handler's own
frames) crosses into the guard page → #DF. Only the double fault itself uses an
IST (IST1). This makes *any* near-full kernel stack a double-fault risk on the
next interrupt — a real, production-relevant bug for any in-kernel code that
uses a lot of stack, not merely a benchmark artifact.

**FIXED part — the 16 KiB gzip hash table (`kernel/src/fs/compress.rs`).**
`lz77_tokenize()` allocated `let mut head = [0u32; HASH_SIZE]` with
`HASH_SIZE = 4096` = **16 KiB on the stack** (a quarter of the whole 64 KiB
stack), while its sibling `prev` was already heap-allocated. Moved `head` to a
`Vec` (heap) and changed `insert_hash`/`find_best_match` to take `&[u32]`/`&mut
[u32]` slices (call sites unchanged — `&mut Vec<u32>` coerces). Verified: with
this fix the `http_gzip_1KiB` and `http_gzip_8KiB` benchmarks now **complete**
(8192B → 4507B), where before they double-faulted. This was the dominant
single stack frame and removing it is correct regardless (gzip should never use
16 KiB of stack).

**OPEN part — RESOLVED 2026-06-15 by the Q7 option-A per-CPU IRQ stack;
empirically confirmed 2026-06-20.** The systemic interrupt-on-near-full-stack
overflow was fixed by moving interrupt handling off the interrupted task's stack
onto a dedicated per-CPU guard-page IRQ stack (`idt.rs::init_irq_stack` /
`run_on_irq_stack` / `IRQ_STACK_TOP`/`IRQ_STACK_BOTTOM`, with nesting-aware
manual RSP switch + `sched::do_deferred_preempt` after RSP is back on the task
stack — see open-questions.md Q7 / design-decisions.md §26). Once IRQ frames no
longer land on a near-full task stack, the 64 KiB task stack is sufficient for
the debug-built `core::fmt`-heavy dashboard path. **Validated 2026-06-20:**
`scripts/boot-test.sh --bench` runs the *entire* deferred suite to completion —
`dashboard_api_status`/`_health`/`_metrics`, `isr_latency`, the 62-entry
scorecard, and a clean `BENCH_OK` — with no double fault (serial-test.txt lines
9843–9913). The stale "still double-faults entering dashboard_api_status"
description below is retained for history only and no longer reproduces.

_Historical (pre-fix) description:_ After the
gzip fix the suite advances one stage further and double-faults again at the
**identical** guard-page `RSP=0xffffc1000003ffb8`, now in `Task 114` during
`bench_dashboard_api_status` (`crate::net::dashboard::bench_api_status`). The
dashboard path has no single large array — it is `format!`-heavy, and debug
builds give `core::fmt` very deep, un-inlined, stack-hungry call chains. So this
is the *general* problem: 64 KiB is marginal for debug-built in-kernel heavy
code + an IRQ frame on top. Fixing it benchmark-by-benchmark is whack-a-mole.

**Proper fix is an architectural decision — see `open-questions.md`.** The
textbook fix is a dedicated per-CPU IRQ stack (x86 IST), like Linux's IRQ
stacks, so interrupt handlers never consume the interrupted task's stack.
**Complication:** the timer handler deliberately re-enables interrupts
(`apic.rs:1162`, `sti` after EOI, for preemption), so IRQs *can* nest — a naive
single shared IRQ IST would be clobbered by a nested IRQ resetting RSP to the
IST top. A correct IRQ-stack implementation must therefore support nesting (or
the hard-IRQ phase must not re-enable IF). This is a careful change to the
hottest, most safety-critical path; alternatives (bump kernel-task stack size;
keep heavy code out of the kernel; release-build) each have tradeoffs. Deferred
to the operator as an open question rather than changing the IRQ path
autonomously.

**Reproduce:** `bash scripts/boot-test.sh --bench --timeout=600`; the suite now
runs through `compress`, `context_switch`, `pick_next`, `ipc`, `vfs`, all
`http_*` incl. both `http_gzip_*`, then #DFs entering `dashboard_api_status`.

**Large-stack-array audit (2026-06-14).** I scanned the kernel for fixed-size
stack arrays ≥ 8 KiB that could contribute to the same overflow class. Findings:
`bench.rs::bench_vfs_throughput_16k` held a `[u8; 16384]` (16 KiB) in the bench
task — moved to a heap `Vec` (committed). Remaining latent (lower-risk, not the
immediate trigger, left as tech-debt): `audio_notify.rs::self_test` `[u8; 8192]`
(boot self-test path), `syscall/linux.rs` ~line 53451 `drain [u8; 8192]`, plus
several `[u8; 4096]` buffers in `rng`/`smp`/`virtio/sound`/`linux.rs` self-tests.
Note these arrays are **not** the immediate dashboard double fault: the
`dashboard_api_status` overflow has **no** large array — it is pure debug-built
`core::fmt` call-chain depth — so reducing stack arrays will not by itself make
`BENCH_OK` appear; only the Q7 IRQ-stack / stack-size decision will.

**Impact (historical):** Before the Q7 IRQ-stack fix, `BENCH_OK` and the last
benchmarks (dashboard API, ISR latency, scorecard) did not complete. As of the
fix (and re-confirmed 2026-06-20) the full deferred suite completes and
`BENCH_OK` prints. Normal operation was never affected: the default `BOOT_OK`
boot test always passed (the deferred bench suite runs only after BOOT_OK).

### W2. Deferred benchmark suite livelocks in `bench_pick_next` after `context_switch` → `BENCH_OK` never prints — ROOT-CAUSED & FIXED 2026-06-14

**RESOLUTION 2026-06-14 — root cause was the mouse cursor task busy-yielding,
NOT a benchmark or backend bug.** The livelock was never about the nop helpers
or `bench_pick_next` per se; it was a **system-wide priority-starvation bug**
that the long bench suite merely exposed first. `cursor_task_entry`
(`kernel/src/mouse.rs`, spawned at priority **16**) polled a lock-free mouse
event ring and, when the buffer was empty, called `crate::sched::yield_now()`
in a tight loop "to avoid spinning." But `yield_now()` re-enqueues the current
task at *its own* priority and then picks the highest-priority Ready task — and
the cursor task, at p16, was *still the highest-priority Ready task*, so it was
immediately re-picked. The "yield" loop therefore **never relinquished the CPU
to any task of priority > 16** (it only ever ceded to something strictly
higher-priority, of which there usually was none). This pinned a core, so every
p≥17 task — the p18 `deferred_bench_task` driver, the p18 workqueue worker,
background daemons — could make progress *only* via the ~1 s anti-starvation
booster (one or two tasks nudged to priority 0 each pass, hence the perpetual
`[sched] Anti-starvation: boosted N tasks` spam). `bench_pick_next` "stalled"
because its driver only got a sliver of CPU per second.

**Diagnosis chain:** markers proved `run()` never returned even though the nop
helpers *did* exit → so the lone driver itself was starving, not the helpers →
boost-ID logging (`cur=<current task> boosted <ids>`) showed the boosted/starved
tasks were tids 115 (bench driver) + 103 (workqueue worker), and that the task
hogging the CPU (`cur=`) was the **mouse cursor task** → reading
`cursor_task_entry` revealed the idle `yield_now()` busy-loop.

**Fix:** in the idle branch the cursor task now `sleep_ms(8)` (~125 Hz) instead
of `yield_now()`. `sleep_ms` (≤100 ms ⇒ hrtimer path) *removes* the task from
the run queue entirely until an hrtimer wakes it, so lower-priority work runs
freely while the cursor is idle; active mouse movement still drains events
tightly (the sleep only triggers once the ring empties). Verified: with this
fix the `--bench` suite runs from `page_alloc` all the way through `compress`,
`context_switch`, `pick_next`, `syscall_dispatch`, `ipc`, `vfs`, and into the
`http_gzip` benchmarks — vastly further than ever before (previously it never
passed `context_switch`). The default `BOOT_OK` boot test still passes
(BOOT_OK after 29 s), confirming no regression to normal operation. (Fixing W2
unmasked a separate latent double fault in a late bench stage — see B-DF1
below.)

**General lesson:** `yield_now()` is NOT a valid "idle until work arrives"
primitive for any task that is not the lowest priority on its core. A task that
yields at its own priority and is the highest-priority Ready task will be
re-picked immediately and spin. Idle waiting must *block* (sleep, or wait on a
waitqueue/futex), removing the task from the run queue. Audit other drivers for
the same `yield_now()`-when-idle antipattern.

---

**Original investigation notes (retained for history):**

**Where:** `kernel/src/bench.rs` `bench_pick_next()` (the
`run("sched_pick_next_4tasks", 500, || sched::yield_now())` loop, run after
the four `bench_nop_task` helpers at priorities 8/12/16/20 are spawned);
interacts with the scheduler's yield/pick path and anti-starvation boost in
`kernel/src/sched/mod.rs`.  Driven from the background `deferred_bench_task`
spawned at the end of `kernel/src/main.rs` boot.

**Symptom:** With `scripts/boot-test.sh --bench --timeout=600`, the deferred
benchmark suite runs cleanly through `page_alloc`, `heap`, `compress`,
`rdtsc`, `hpet`, and `context_switch_rt`, then **stalls** at/after
`bench_pick_next`: no `[bench] sched_pick_next_4tasks: …` line is ever
printed, `BENCH_OK` never arrives, and the serial log fills with continuous
`[sched] Anti-starvation: boosted N tasks to priority 0` (N = 1–2).  The
default `BOOT_OK` boot test is unaffected (it stops at `BOOT_OK`, long before
the benchmarks run).

**CORRECTION 2026-06-14 — the four nop helpers DO exit (original
"never exit" claim falsified).** A 600 s-timeout run captured all four
`bench-pn` nop helper tasks (tids **119, 120, 121, 122**) printing
`[sched] Task N exiting` *after* `context_switch_rt`'s result line — i.e. they
spawn AND drain to `task_exit` successfully.  So the nop helpers are **not**
the livelocking tasks, and `bench_pick_next`'s task-draining works.  The hang
is therefore **after** the helpers exit: either `run("sched_pick_next_4tasks",
500, yield_now)` not returning on the lone driver task (tid 114) once the
helpers are gone, a *later* benchmark stage that the driver enters silently, or
genuine starvation of 1–2 **other** Ready tasks (background daemons / the
workqueue worker tid 104 at prio 18) behind the busy prio-18 driver — those
are what the perpetual "boosted 1–2 tasks" lines refer to, NOT the nop
helpers.  Next diagnosis must localize where tid 114 actually gets stuck after
the helpers drain (add a marker after `run()` returns in `bench_pick_next` and
at the start of `bench_syscall_dispatch`), rather than assuming the nop helpers
are the culprit.

**Assessment:** Independent of the F15 sleep-queue leak — it reproduced
identically *before* the F15 fix (when it could have been blamed on
kswapd/workqueue spin-starvation) and *after* it (0 `sleep queue full`
warnings).  `run()` is a plain non-blocking loop and the task-exit path
(`task_finished` → `task_exit` → `schedule_inner(false, Uncounted)`) is clean,
so the hang is a scheduler-level livelock among several equal-/mixed-priority
tasks that only `yield_now()` (no sleeping, no I/O).  The persistent
anti-starvation boosting suggests the scheduler is thrashing — repeatedly
boosting starved tasks to priority 0 without the nop helpers ever being
scheduled through to completion.  Not yet root-caused.

**Impact:** The deferred micro-benchmark suite cannot complete past
`context_switch`, so `BENCH_OK` and the later benchmarks (pick_next, syscall
dispatch, IPC, VFS, net, crypto, HTTP, ISR latency, scorecard) never run in
normal operation.  Early-benchmark perf tracking still works:
`boot-test.sh --bench` prints the captured numbers up to the hang even on
timeout.

**Update 2026-06-14 (anti-starvation duplicate-enqueue fix — ruled OUT as the
root cause):** While investigating, I found and fixed a genuine
scheduler-correctness bug in the anti-starvation booster
(`check_starvation()` in `kernel/src/sched/mod.rs`): it boosted a starved
Ready task by `PER_CPU_SCHED.dequeue(id, effective_priority(), cpu)` followed
by `enqueue(id, 0, cpu)`.  Because `effective_priority()` returns the task's
*base* priority while an already-boosted task physically sits in priority
queue 0, the level-targeted dequeue scanned the wrong queue, removed nothing,
and the enqueue created a **duplicate** run-queue entry — the same task id
present twice in queue 0.  Re-boosting on every ~1 s pass (the booster never
reset `ready_since_tick`) multiplied the duplicates without bound.  Fix:
(a) added `dequeue_any(id)` to `PriorityRoundRobin`/EEVDF/Deadline +
`SchedulerBackend`/`PerCpuScheduler`, which removes *all* copies of a task at
*any* level and clears the bitmap bit when a level empties; the booster now
`dequeue_any` then single-`enqueue` at 0, leaving exactly one entry; and
(b) the booster now resets each boosted task's `ready_since_tick` so it is not
re-boosted before being dispatched.  This is a real, system-wide fix (the
corruption could happen to any starved task, not just benchmark tasks).
**However, it did NOT resolve W2:** with the fix in place the suite still
stalls entering `bench_pick_next` (no `sched_pick_next_4tasks` line, `BENCH_OK`
never arrives), boot remains clean (0 self-test failures, 0 sleep-queue spin
warnings), and the booster still fires (now without duplicating entries).  So
the duplicate enqueue was an *amplifier* of the thrash, not the trigger: the
benchmark nop helpers still genuinely fail to run to `task_exit`.

**Timeout calibration (corrected — the original stall-point stands).** A first
post-fix run with the default 300 s timeout appeared to stall right after
`heap_raw_alloc_free_4096`, suggesting the hang had moved earlier.  That was a
**timeout artifact, not a regression**: a 600 s re-run showed the suite *does*
still progress cleanly through `compress`, `rdtsc`, `hpet`, and
`context_switch_rt`, then stalls entering `bench_pick_next` — exactly the
original symptom.  The 300 s budget simply expired *inside* the
`compress_repeating` benchmark, which is savagely slow under QEMU/TCG:
mean ≈ 1.01 s per iteration × 200 iters ≈ **~202 s for that one benchmark
alone** (max single iter ≈ 22 s).  Because `bench_pick_next`'s own work is
trivial (~110 ms for all 500 yields at the measured ~220 µs/round-trip), its
failure to complete within the remaining multi-hundred-second budget confirms a
**genuine stall**, not mere slowness.  Practical note: reproduce W2 with
`scripts/boot-test.sh --bench --timeout=600` (the default 300 s no longer
reaches the stall point because the compress benchmarks eat the budget first).

The deeper root trigger (why four `yield_now()`-only tasks at priorities
8/12/16/20 never drain past `bench_pick_next`) is still uncharacterised.

**Next step:** Add finer-grained serial markers inside `bench_pick_next`
(before/after spawn, before/after the `run()` loop, per-iteration sampling)
and instrument the scheduler's pick/yield path to capture *which* task is
selected each switch during the stall.  Determine whether the nop helpers are
never picked, or are picked but never run to their `task_exit`.  Likely a
priority/round-robin or anti-starvation interaction; treat as a real
scheduler-correctness bug, not merely a benchmark quirk.  Risky to change the
scheduler blindly, so diagnose before patching.

_(The two prior watchlist items — accounting
self-test hang and invariant self-test hang — went 90 consecutive
boot tests with zero recurrence after F4/F5 and have been closed as
"likely cured incidentally," and as of 2026-06-10 a further 38 clean
boots (128/128 total) keep them closed.  See F6 and F7 in Fixed Bugs.
The two items discovered 2026-06-10 — quota Test 5 and FS interceptor
deny — are now fixed; see F8 and F9.)_

### TD-BENCH-OWNER-AB-BUDGET-WAS-AN-ABSOLUTE-CYCLE-COUNT. Five boots of "ownership tagging costs 8500 cycles" were the emulator, not the code — 2026-08-14 — RESOLVED 2026-08-14

**Where:** `kernel/src/bench.rs` (`run_all`, the `page_alloc_free_owner_ab` and
`fast_cpu_index` budget checks) and `bench/baselines.toml`.

**Symptom.** `page_alloc_free_owner_ab` reported `SLOW (tagging costs N
cycles/alloc+free, limit 500)` on five consecutive boots: **10826 / 7660 /
8512 / 10580 / 11288**. `fast_cpu_index` simultaneously reported `SLOW (274 /
282 cycles, limit 200)` — on boots *after* the tier-0 fix that benchmark had
been added to prove worked.

**Why it was worth chasing rather than dismissing as noise.** The
reproducibility. A number that lands within ±20% five times is measuring
something. And the accused code is trivial: `frame_owner::set` is a relaxed
load, a bounds check, a byte store and a counter bump. When a measurement and
a static reading of the code disagree by two orders of magnitude, one of them
is wrong, and guessing which is how people end up optimising the wrong line.

**Three hypotheses, each killed by measurement rather than by argument:**

1. *Ambient load / windowing.* The first version ran 500 iterations with
   tagging off, then 500 with it on. Two consecutive windows on a live system
   are not the same system, and `min` does not save you — it is robust to
   *spikes*, not to a window uniformly busier than its neighbour. The evidence
   was in the same output: the off window had `max=129078` while the on window
   had `max=635531436` and a 30x higher mean. Fixed by alternating the arms
   every iteration (`ab_interleaved`), microseconds apart, so drift on a
   scheduling timescale lifts both and cancels. **The number did not move.**
2. *TCG's atomic-RMW fallback.* TCG cannot always lower a guest atomic RMW
   inline; `cpu_loop_exit_atomic` aborts the translation block and re-executes
   with the world stopped — thousands of cycles for one increment. Two shared
   `fetch_add` statistics counters sat on exactly this path. Measured directly:
   `atomic_fetch_add_relaxed` came out at **0-238 cycles**, so the two counters
   accounted for ~124 of ~8500. (The counters were moved to per-CPU
   cache-line-padded slots anyway, on general principles — the file already
   padded `CURRENT_OWNER` per-CPU with a comment about a "false-sharing storm"
   while leaving these unpadded on the same path. That commit says explicitly
   that it was *not* the cause.)
3. *The halves don't add up.* A first split put `set` at 2978 cycles and
   `current_owner` at 924 — `set + clear + current ≈ 6900` against a measured
   8512-10580, so they did add up and the cost was genuinely inside `set`.
   **But that split was flawed**: it never controlled the `ENABLED` flag, so
   "2978 cycles for `set`" lumped the cost of *calling* `set` together with the
   cost of the work `set` does — the two things that had to be told apart,
   pointing at opposite conclusions.

**What actually settled it.** A three-arm experiment: `set` with tracking
**off** (the early-return path — the harness's floor for calling into
`frame_owner`), `set` with tracking **on**, and a byte store to an ordinary
`.bss` static as a control. Result:

```
frame_owner_set_split: call_floor=278 work=2416 bss_store_control=218
```

**A single byte store to plain kernel `.bss` costs 218 cycles in this
harness.** `set` performs about half a dozen guest memory accesses (the
`ENABLED` load, the length and pointer loads inside `slot`, the tag store, and
the per-CPU counter's load and store); 6 × 218 ≈ 1300, the right order for the
measured 2416. Under TCG *every* guest memory access carries a softmmu lookup
costing a few hundred host cycles; the same accesses on real hardware are L1
hits at ~1-4 cycles. So ownership tagging adds ~16 memory accesses per
alloc+free — **~30 cycles of real machine, ~2500 cycles of emulator.**

Nothing had regressed. The benchmark was measuring the emulator and comparing
it to a budget sized for hardware.

**The real defect, and it is a general one.** An absolute cycle budget cannot
work in this harness. It conflates the code under test with an emulation
constant that varies with the host, the QEMU build and the accelerator, and it
fails permanently on code that is correct. `fast_cpu_index`'s 200-cycle budget
was the same defect in its purest form: **200 cycles is below the harness's
floor for a single memory access**, so *no* implementation could ever have
passed it — the check was structurally incapable of reporting PASS, and it was
accusing the very fix it had been added to guard.

**Fix — budgets in units of measured memory accesses.** `run_all` now measures
`memory_access_floor` first (a byte store to a dedicated `.bss` static,
interleaved against an empty closure, clamped to a minimum of 100 so a
noise-driven 0 cannot collapse every budget to 0) and expresses both delta
budgets as multiples of it:

* `fast_cpu_index`: 4 accesses (was 200 cycles). Clear of the noise, still far
  under an APIC MMIO round-trip.
* `page_alloc_free_owner_ab`: 40 accesses (was 500 cycles). The path performs
  ~16, so 2.5x headroom absorbs variation in how many the optimiser folds.

**The fix failed its own verification boot, in two ways, and both are worth
recording because both are mistakes the *unit change* made easy to miss.**

```
memory_access_floor: 100 cycles/guest byte-store (measured=74 nop=1278 store=1352)
fast_cpu_index: PASS (288 cycles over an empty closure, limit 400 = 4 accesses)
page_alloc_free_owner_ab: SLOW (17176 cycles/alloc+free = 171 accesses, limit 40)
```

1. **The calibration was itself vulnerable to the noise it existed to
   correct for.** It timed *one* store against one empty closure, and the
   subtraction is only meaningful if the two arms' baselines agree. They did
   not: `nop=1278` here, while the very next block in the same run measured
   `nop=448`. A ~200-cycle access simply has no signal above a
   several-hundred-cycle wander, so `measured=74` was noise and **the clamp
   became the answer** — which then under-scaled every budget derived from it
   and manufactured the SLOW below. Fixed by *amplifying*: 64 stores per timed
   window, divided by 64. The signal scales with N, the wander does not, so it
   divides away. The loop's own overhead is left inside the measurement
   deliberately — it can only enlarge the floor and loosen the budgets, and for
   a check whose whole purpose is to stop crying wolf, false negatives are the
   safe direction. The clamp stays as a backstop, but if a run ever prints
   `measured` at or below it, that run's budget verdicts are unreliable rather
   than findings.
2. **40 accesses was too tight even against a correct floor.** Honest recount:
   ~20 architectural accesses per alloc+free (`tag_alloc_owner` = 1 is_enabled
   + 2 current_owner + 8 set; `untag_free_owner` = 1 + 8). Observed healthy is
   ~50-57 (11288/218 = 51.7 on one boot, ~57 on the next) — a consistent
   2.5-3x multiplier, which has a cause rather than being slop:
   `scripts/boot-test.sh` runs a plain `cargo build`, and the workspace's
   `[profile.dev]` sets only `panic = "abort"`, so **opt-level is 0 and the
   benchmarked kernel is unoptimised** (`cargo` prints `Finished dev profile
   [unoptimized + debuginfo]`). Nothing is inlined, so each of the ~6 calls on
   this path runs a real prologue/epilogue whose spills and saved registers are
   memory accesses the source-level count omits. ~3x over the architectural
   count is what an unoptimised build predicts and what two independent boots
   measured. Budget raised to **150** ≈ 3x the observed ~50.

   The temptation here was to loosen until it passes, which is the anti-pattern
   this whole entry is about. What makes 150 legitimate is that the number is
   derived from a *mechanism* (opt-level 0, non-inlined calls) that predicts the
   observed multiplier independently, and that the looseness costs no detection
   power: this is a structural tripwire, not a stopwatch, and every failure it
   guards against is an order-of-magnitude event, not a percentage.

**Verified 2026-08-14** on the boot following both follow-up fixes:

```
memory_access_floor: 284 cycles/guest byte-store (measured=284 over 64 stores/window: nop=8238 store=26474)
fast_cpu_index: PASS (476 cycles over an empty closure, limit 1136 = 4 accesses)
page_alloc_free_owner_ab: PASS (tagging costs 11778 cycles/alloc+free = 41 accesses, limit 150)
frame_owner_set_split: call_floor=282 cycles work=3054 cycles
```

The amplified calibration produced `measured=284` where the single-store form
produced `74`, so the clamp no longer binds and the floor is a real quantity.
It cross-checks: the independently-measured `.bss` control in
`frame_owner_set_split` came out at 218 on an earlier boot, and 284 exceeds
that by roughly the loop overhead this deliberately declines to subtract.

The detail worth keeping in view is that the absolute cycle figure — **11778** —
sits squarely in the same 7660-11288 band that was reported as `SLOW` for five
consecutive boots. Not one line of `frame_owner` changed between those runs and
this one. Only the unit the budget is written in changed, which is the whole
thesis of this entry stated as a measurement.

**Follow-up wart, also fixed:** the split diagnostic printed
`call_floor=282 cycles (0 accesses)` — 0.99 accesses truncated to `0` by
integer division, reading as "this costs nothing" in the one line whose job is
to say where the cost lives. Access counts now print to one decimal via a
`accesses(cycles, floor) -> (whole, tenths)` helper.

This keeps the checks doing what they exist for. The failures worth catching —
an uncached MMIO round-trip, a contended lock, a per-frame loop where a
`write_bytes` belongs (which scales with `count`) — cost 10-100x a plain
access on hardware *and* under emulation, so they still blow past the budget.

**Kept as a permanent diagnostic:** the `frame_owner_set_split` line, now
reported in access units. If the A/B ever fires again it says in one line
whether the cost is inside `set`'s working path or elsewhere — the fork this
investigation burned four boots failing to resolve by argument.

**Lesson for the next benchmark added to this file.** Any threshold on an
in-kernel QEMU/TCG measurement must be expressed relative to something
measured by the same harness in the same run. Absolute nanosecond and cycle
targets taken from Linux publications belong in `baselines.toml` as *context*;
they cannot be pass/fail gates here. The pre-existing `ABOVE TARGET` verdicts
in this suite (e.g. `isr_latency: 233451ns, target 10000ns, 2334%`) are the
same category of statement and should be read as "this is what the emulator
does", not as regressions.

---

### B-SPAWN-SYSCALLS-NEVER-RECORDED-THE-PARENT. Every syscall-spawned child was unreapable, uncapability'd, and escaped its parent's namespace — 2026-08-14 — FIXED 2026-08-14

**Where:** `kernel/src/syscall/handlers.rs` — `sys_process_spawn` (~3279) and
`sys_process_spawn_ex` (~3397). Fallout in `kernel/src/proc/spawn.rs`
(`SpawnOptions::parent`, steps 5b/5c) and `kernel/src/proc/pcb.rs`
(`try_reap`, 4219-4247).

**Symptom that led here.** The `ticker` service crash-looped nine times in a
routine boot log, each time reported by init as `exited with code -400` — while
`[ticker] Ready.` kept appearing in the same log. A process cannot both be
dead and printing. `-400` is `KernelError::PermissionDenied`, and the only
permission check on the wait path is `pcb::try_reap`'s
`proc.parent != parent_pid`.

**Root cause.** `SpawnOptions` has a `parent(pid)` builder, and **neither spawn
syscall ever called it**. Both built `SpawnOptions::new(name)` and left
`options.parent = 0`. So every process ever created through
`SYS_PROCESS_SPAWN` / `SYS_PROCESS_SPAWN_EX` recorded the kernel (PID 0) as its
parent. Three consequences, all silent:

1. **No process could reap its own children.** `try_reap` compares the caller
   against the recorded parent and returns `PermissionDenied`. Every spawned
   child therefore leaked as a zombie for the life of the system, and every
   supervisor mis-read the error as an exit status (see the lane-b request
   `requests/a-b-init-conflates-syscall-error-with-exit-code.md` — init prints
   `ret` from `process_try_wait` as an exit code without checking its sign,
   which is how `-400` became "exited with code -400" and triggered a restart).
2. **The parent was never granted a `Process` capability over the child.**
   `spawn_process` step 5b (spawn.rs:974) is gated on `options.parent != 0`, so
   the READ|WRITE|DELETE|WAIT|SIGNAL|DUPLICATE grant never happened. Callers
   had no handle with which to signal or kill what they had spawned.
3. **Sandbox escape.** Step 5c (spawn.rs:1001) inherits the parent's filesystem
   namespace, and is gated on the same condition. A process confined to a
   non-root namespace could spawn a child that landed in the *root* namespace.
   This is the serious one: confinement was defeated by the single act of
   spawning.

**Why nobody noticed.** `fork` sets the parent on its own path, so every
fork→exec→wait test passed. Nothing in the test suite spawned via the syscall
*and then reaped*. And `SpawnOptions::parent` carried
`#[allow(dead_code)] // Public builder API — callers use SpawnOptions::new() + chaining.`
— a suppression that was factually wrong (there were no chaining callers) and
that turned "this builder method has zero callers" from a compiler warning into
a comment asserting the opposite. That is the real lesson: an `#[allow]` whose
justification is a claim about the rest of the codebase silently rots when the
claim stops being true.

**Fix.** Both syscalls now pass `.parent(caller_pid().unwrap_or(0))`.
`unwrap_or(0)` is correct rather than a fallback: no caller PID means the spawn
came from the kernel, and PID 0 has implicit authority and needs no grant. The
`#[allow(dead_code)]` on `SpawnOptions::parent` is removed — if it ever goes
unused again that should be a warning, not a comment.

**Regression test.** `test_spawn_records_parent` in `kernel/src/proc/spawn.rs`
(registered in `spawn::self_test`) spawns with an explicit parent, asserts
`pcb::parent(child) == Some(parent)`, then asserts `try_reap(parent, child)` is
**not** `PermissionDenied`. It deliberately accepts "still running" as a pass:
the bug produced a specific error code, and testing for its absence is what
distinguishes the fix from emulator timing.

### B-FAST-CPU-INDEX-FELL-BACK-TO-AN-APIC-MMIO-READ-ON-EVERY-ALLOC. A self-inflicted allocator regression, and the benchmark that should have caught it never runs — 2026-08-14 — FIXED 2026-08-14

**Where:** `kernel/src/smp.rs` — `fast_cpu_index` / `current_cpu_index`.

**Self-reported.** Nothing failed; I found this by re-reading my own
`TD-FRAME-OWNER-1GIB` change against CLAUDE.md's performance-critical table.
It is logged rather than quietly patched precisely because it was invisible.

**What I did wrong.** `TD-FRAME-OWNER-1GIB` wired ownership tagging into the
allocator, so `alloc_frame` and `free_frame` each gained a call to
`frame_owner::current_owner()` → `smp::fast_cpu_index()`. On the boot-test CPU
model (`qemu64`) **neither RDPID nor rdtscp is advertised**, so every one of
those calls fell through to tier 3 — an uncached APIC MMIO read, hundreds of
cycles under emulation. `alloc_frame` is in the performance-critical table
(Linux buddy 100-500 ns, our target < 1 µs) and the recorded QEMU baseline for
`page_alloc_free` is 198 ns / 736 cycles, so this was a large relative cost on
a path CLAUDE.md explicitly says to benchmark after every change. I merged it
without benchmarking.

**The same fallback was also in ISR context.** `current_cpu_index` carried its
own hand-copied duplicate of the tier ladder, and the copy had drifted: it
never grew the RDPID tier, so on RDPID hardware it paid for a TSC read it threw
away, and on `qemu64` it took the APIC MMIO round-trip **on every timer tick**.
Its doc comment names the timer ISR as a hot path, which is what makes the
drift notable — the duplication defeated the optimisation exactly where the
comment claimed it mattered.

**Fix — tier 0, plus deleting the duplicate.** `fast_cpu_index` gained a
tier-0 fast path guarded by a new `MULTI_CPU_ACTIVE` flag: while no AP has ever
been released from the trampoline, exactly one CPU is executing kernel code, so
the answer is provably `BSP_CPU_INDEX` and no hardware read is needed at all.
`current_cpu_index` now delegates to `fast_cpu_index` after its
`SMP_INITIALIZED` gate, so there is one ladder instead of two and it cannot
drift again.

**The flag is deliberately not `NUM_CPUS_ONLINE > 1`,** which is the obvious
implementation and is unsound. An AP runs `gdt::init_for_ap`, `apic::init_ap`
and `spectre::init_ap` — all of which allocate — *before* it bumps that counter
(`smp.rs`, `ap_entry`). A counter-based test would therefore tell a live AP it
was the BSP and hand it the BSP's per-CPU allocator magazine: silent cross-CPU
corruption. Instead the **BSP** sets `MULTI_CPU_ACTIVE` before it sends the
first INIT-SIPI, so it is already true before an AP retires its first
instruction. It is monotonic and never cleared, because a stale `true` merely
costs the normal hardware read whereas a stale `false` is unsound.

**The real finding is why no one noticed.** The project has a benchmark suite,
`bench/baselines.toml` targets, and a pass/fail scorecard — and
`bench::run_all()` is spawned as a *deferred low-priority task* that prints
`BENCH_OK` only after `BOOT_OK`. The routine boot test waits for `BOOT_OK` and
kills QEMU immediately, so the benchmarks never finish. In the 26094-line KASAN
log, `[bench] === Kernel micro-benchmarks ===` is the **second-to-last line** —
the task started and was killed mid-suite. In the ordinary boot log the header
does not appear at all. So there was no gate to catch this, and there is none
for the next one either. `boot-test.sh --bench` (which waits for `BENCH_OK` and
surfaces `ABOVE TARGET` verdicts) exists but is not part of the routine gate.
Tracked separately as `TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE`.

**Regression guard added.** `bench::run_all` now measures `fast_cpu_index`
directly, with a `fast_cpu_index` entry in `bench/baselines.toml`. The target
(100 ns) is deliberately loose: it exists to detect "we fell back to the APIC
MMIO path", not to police single cycles. It is benchmarked not for its own sake
but because it is a *multiplier* — called twice per frame alloc/free and twice
per heap alloc/free — so a regression in it surfaces as a diffuse slowdown
across the whole allocator rather than as an obvious local fault, which is
exactly how this one hid.

---

### B-MKDIR-ALL-BUILT-A-RELATIVE-PATH-FROM-AN-ABSOLUTE-ONE. Every `mkdir -p` in the kernel failed `InvalidArgument`; boot panicked mounting `/tmp` — 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/vfs.rs`, `Vfs::mkdir_all` (~line 2217); the same bug
in `kernel/src/fs/pathbar.rs`, `parse_breadcrumbs`.

**Symptom:** the QEMU boot test died at
`panicked at kernel\src\container.rs:6996:40: add /tmp tmpfs: InvalidArgument`.
Not a `/tmp`-specific fault: *every* caller of `mkdir_all` was broken, and
`container.rs` was simply the first one on the boot path that treats the
failure as fatal.

**Root cause — the one sharp edge of `Path::components()`.** `components()`
drops empty components, which is exactly what makes it robust against `//`,
a trailing `/`, and `.`-free normalisation. The corollary is easy to miss:
the leading `/` of an absolute path *is* an empty leading component, so it is
dropped too. `components()` on `/a/b` yields `a`, `b` — nothing that says
"absolute".

`mkdir_all` rebuilt the prefix chain by pushing components onto a fresh
`PathBuf::new()`:

```rust
let mut built = PathBuf::new();          // WRONG
for comp in &components { built.push(comp); ... Self::stat(&built) ... }
```

so the first probe was `stat("a")`, a *relative* path, which
`validate_path` rejects with `InvalidArgument` ("must be absolute") before
any filesystem is consulted. The function could never get past its first
component.

**Fix:** seed the accumulator with the root separator, with a comment naming
the trap so the next reader does not re-introduce it:

```rust
// Seed with the root separator, not an empty buffer: `components()`
// drops the leading `/`, so pushing the first component onto an empty
// `PathBuf` would build a *relative* path and the `stat` below would
// fail `validate_path` ("must be absolute") before touching the disk.
let mut built = PathBuf::with_capacity(norm.len().saturating_add(1));
built.extend_bytes(b"/");
```

`pathbar::parse_breadcrumbs` had the identical shape and emitted relative
breadcrumb paths (`home`, `home/user`) for an absolute input; it is now seeded
with `PathBuf::from(if normalized.is_absolute() { "/" } else { "" })`.

**Audit.** All 19 `components()` rebuild loops in the kernel were checked
after this. The rest are either correctly seeded (`cap/file_tags.rs` ×2,
`vfs.rs::normalize_path`, `ipc/namespace.rs::normalize_jailed`,
`memfs.rs::parent_path_of`) or deliberately relative (`overlay.rs` ×3,
`container.rs:3106`, `oci.rs::archive_norm`, `oci.rs:1154`, `cpio.rs:349`,
`pathutil.rs:121`, `kshell.rs:79987`). **This is a recurring trap, not a
one-off** — treat "rebuilding a path from `components()`" as a code smell and
check the seed every time.

### B-CPIO-DROPS-NON-UTF8-MEMBER-NAMES. A cpio member whose name is not UTF-8 vanished from listings and was never extracted — 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/cpio.rs`, the name-decoding step of `parse`.

**What it was:** `CpioEntry::name` was a `String`, so the parser decoded the
NUL-terminated byte field with `core::str::from_utf8(...)` and, on failure,
substituted `""`. An empty name matches nothing, is filtered out of listings,
and names no file on extraction — so the member was *silently* dropped. Since
cpio's name field is a raw byte run terminated by NUL (any byte but NUL is
legal, exactly like our own paths), this is a routine input, not an exotic
one: any archive built on a non-UTF-8 locale or containing a file that came
off a foreign filesystem hits it.

**Consequence:** `cpio -t` under-reported the archive's contents with no
diagnostic, and `cpio -i` extracted fewer files than the archive held while
reporting success. An initramfs built with such a name would be missing that
file at runtime.

**Fix:** `CpioEntry::name` and `::link_target` are now `fs::path::PathBuf`, so
the bytes are carried through unmodified and no decode happens at all. Part of
`D-VFS-PATHS-ARE-STR-NOT-BYTES`; see
`TD-ARCHIVE-WRITER-NAMES-ARE-STRING-NOT-BYTES` for the formats still to
convert.

### B-ZIP-FABRICATES-MEMBER-NAMES. A ZIP member whose name is not UTF-8 was extracted under an invented `<invalid-utf8@0x…>` name — 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/zip.rs`, the central-directory / local-header name
decode in `parse`.

**What it was:** `ZipEntry::name` was a `String`, and where the stored bytes
did not decode as UTF-8 the parser synthesised a placeholder of the form
`<invalid-utf8@0x…>` (the offset of the record) and used it as the member's
name. Unlike the cpio bug this is not a *drop* — it is worse. The placeholder
is a perfectly usable name, so:

- `unzip -l` displayed a name that appears nowhere in the archive;
- `unzip` **wrote the file out under the fabricated name**, so extraction
  produced a file that was not the one the archive contained, with no error
  and no warning;
- round-tripping (extract, re-zip) permanently replaced the real name with the
  placeholder.

ZIP does not require UTF-8. General-purpose bit 11 only *claims* it; DOS/CP437
and arbitrary locale bytes are ubiquitous in real archives, so this fires on
ordinary third-party input.

**Fix:** `ZipEntry::name`, `ZipWriteEntry::name` and `DirRecord::name` are now
`fs::path::PathBuf`; the parser does `PathBuf::from(name_bytes)` and the
directory-member test became a byte test
(`name.as_bytes().ends_with(b"/")`) rather than a `char` test. Display sites
use `.display()`, which is lossy *for the terminal only* and never feeds back
into a filename.

### B-RAR-DROPS-NON-UTF8-MEMBER-NAMES. A RAR5 member whose name is not UTF-8 parsed as `""` — 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/rar.rs`, the file-header name decode in `parse`.

**What it was:** `RarEntry::name` was a `String` and the parser did
`core::str::from_utf8(bytes).unwrap_or("")`. RAR5 nominally specifies UTF-8 in
the name field, but the field is a length-prefixed byte run that *nothing*
validates — neither the format's own CRC (which covers the header bytes, not
their encoding) nor our parser — so an archive written by a tool that stored
locale bytes yields raw non-UTF-8 there routinely.

**Consequence:** the same shape as the cpio bug and just as silent. `unrar -l`
listed the member with an empty name; `unrar` fed `""` to
`pathutil::confine_under`, so the member either vanished or, for several such
members, they all collided on one name — the last one written won. No
diagnostic in either case.

**Fix:** `RarEntry::name` is a `PathBuf` built with `PathBuf::from(bytes)`; no
decode happens at all. The self-test's name comparisons became
`name.as_path() != Path::new("…")` and its prints `.display()`.

### B-7Z-COLLAPSES-UNPAIRED-SURROGATES-IN-MEMBER-NAMES. Distinct 7z members collided on one name — 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/sevenz.rs`, the `K_NAME` property decode in the
header parser.

**What it was:** 7z stores names as NUL-terminated UTF-16LE, and the parser did
`String::from_utf16_lossy(&name_u16)`. "Lossy" here means *every* unpaired
surrogate becomes U+FFFD. Windows filenames are UTF-16 with no well-formedness
requirement, so unpaired surrogates are legal on the filesystem the archive was
most likely built on. Two members named `a\u{D800}.txt` and `a\u{DC00}.txt` are
distinct files on disk but decoded to the *same* string here.

**Consequence:** the listing showed two identical names, and extraction wrote
one file twice — the second silently overwriting the first, so `un7z` reported
success having lost a file's contents. The old code also ran
`name.replace('\\', "/")` *after* the lossy decode.

**Fix:** added `PathBuf::from_utf16` — a proper UTF-16 → **WTF-8** encoder
(UTF-8 extended so an unpaired surrogate encodes as its own 3-byte sequence),
which is lossless and so keeps those two names distinct. `SevenZEntry::name`
and `FileInfo::name` are `PathBuf`. The `\` → `/` normalisation now runs on the
`u16` code units *before* the conversion, where it is a single code-unit
comparison that cannot touch a byte inside a multi-byte sequence.

### BUG-TRYWAKE-FALSE-CONFLATES-CONTENTION. `sched::try_wake` returned `false` both for "lock contended, retry" and for "already recorded as `pending_wake`", so every wake aimed at a not-yet-parked task queued a duplicate deferred wake that later fired against an unrelated park — FIXED 2026-07-27

**Where:** `kernel/src/sched/mod.rs` — `try_wake()` (~1874) and its ~20 call
sites, all of which spell the same idiom:

```rust
if !sched::try_wake(tid) {
    sched::defer_wake(tid);
}
```

(ipc: pipe, eventfd, stream_socket, channel, futex ×4, semaphore, service,
completion, timerfd; plus proc::signal ×2, fs::notify, ioapic, syscall::linux
×2, and `process_deferred_wakes` itself.)

**Bug.** `try_wake` returned `false` in three materially different
situations, and the idiom above cannot tell them apart:

1. `SCHED.try_lock()` failed — nothing was done, retry is *required*;
2. the lock was taken but the task was `Running`/`Ready`, so `pending_wake`
   was set — the wake is **fully accounted for**, retry is *wrong*;
3. the task did not exist — nothing to wake, retry can *never* succeed.

Case 2 is not a corner case: it is the ordinary register-then-recheck
interleaving, i.e. any wake that lands before the target has actually parked.
Every such wake therefore also enqueued a **duplicate** into the 32-slot
deferred-wake queue. That duplicate then fired later against whatever the
task was doing at the time — waking an unrelated, *subsequent* park early via
`drain_deferred_wakes_locked`, or (via `process_deferred_wakes`) re-setting
the sticky `pending_wake` bit so some other `block_current()` returned
without blocking. This is precisely the "stale `pending_wake` consumed by an
unrelated later park" lead recorded under
`BUG-DASH-CMDSUB-INTERMITTENT-HANG`.

Case 3 leaked resources outright: `process_deferred_wakes` only frees a slot
when `try_wake` returns `true`, so a deferred wake for a dead task could
never be cleared. One leaked slot pinned `DEFERRED_WAKES_PENDING` forever,
forcing a full 32-slot rescan on every timer tick, and enough of them would
exhaust the queue.

**Impact.** Spurious early returns from `block_current()`. Park loops that
re-check their condition degrade to a harmless spin, which is why this stayed
latent; any single-shot `block_current()` would proceed as if its event had
occurred.

**Found by:** following lead (b) of `BUG-DASH-CMDSUB-INTERMITTENT-HANG` after
the single-waiter-slot sweep (lead (a)) closed.

**Fix.** `try_wake`'s contract is now unambiguous: it returns `false`
**only** for case 1, and `true` for every case in which the wake has been
accounted for (2 and 3 included). No call site needed to change — the
existing `if !try_wake { defer_wake }` idiom becomes exactly correct, and
`process_deferred_wakes` now frees slots that could previously never be
freed. The body was also flattened to `let … else` so each case returns
explicitly instead of falling through to a shared trailing `false`, which is
what allowed the three to be conflated in the first place.

This is the same fix, and the same shape, as the one already applied to the
*sleeper* queue: `wake_expired_sleeper()` (~4949) returns
`SleeperWake::Retry` **only** on lock contention, and `SleeperWake::Release`
for both the not-blocked and the task-gone cases, after an identical slot
leak. The deferred-wake queue had simply not been given the same treatment.

**Regression test:** `test_try_wake_contract()` in `kernel/src/sched/mod.rs`
(wired into `sched::self_test()`). Asserts `try_wake` on a non-existent task
returns `true`; asserts `try_wake` on the *running* caller returns `true`,
then that the resulting `pending_wake` token makes the very next
`block_current()` return instead of parking, then that the token was consumed
(read directly out of the task table rather than by parking again, which
would genuinely block). Boot-verified: `[sched]   try_wake contract (retry
only on lock contention): OK`.

### BUG-SLEEP-RETURNS-EARLY-ON-ANY-WAKE. `sleep_ns`/`sleep_until_tick` parked once and returned, so *any* wake — an unrelated `sched::wake`, or a stale `pending_wake` token — ended the sleep early: a `sleep(5s)` could return after a millisecond — FIXED 2026-07-27

**Where:** `kernel/src/sched/mod.rs` — `sleep_until_tick()` (~4566) and
`sleep_ns()` (~4876), plus their `sleep_ms`/`sleep_us` wrappers.

**Bug.** Both arm a timer (a sleep-queue slot / an hrtimer), call
`block_current()` **once**, and return. `block_current()` returning is not
evidence that the deadline arrived: any `wake`/`try_wake` aimed at the task
releases it, including a stale `pending_wake` token (see
`BUG-TRYWAKE-FALSE-CONFLATES-CONTENTION`) and the `pending_wake` that
`wake_expired_sleeper` deliberately sets when it finds an early-woken
sleeper. So the sleep duration was a *maximum*, not a duration — while every
caller reads it as a duration:

* `sys_sleep` (`handlers.rs`) and `nanosleep`'s no-signal-context fallback
  (`linux.rs`, whose own doc comment calls `sleep_ns` "non-interruptible");
* io_ring `IO_OP_TIMEOUT`, which promises "sleep for the specified duration";
* `sched::supervisor` and `ktimer` self-tests, which sleep to let real ticks
  elapse and would otherwise assert against a clock that had not moved.

**Fix — split the contract instead of picking one.** This mirrors Linux,
where `schedule_timeout()` may return early and `msleep()` loops around it:

* `sleep_ns` / `sleep_until_tick` / `sleep_ms` now **loop on the clock**,
  re-arming for the *remaining* time until the deadline genuinely arrives.
  The `sleep_ns` deadline is captured *before* arming the hrtimer, so it can
  never be later than the timer's own expiry — a real timer wake can't be
  mistaken for a spurious one and re-parked against a timer that already
  fired.
* `sleep_ns_interruptible` / `sleep_until_tick_interruptible` /
  `sleep_ms_interruptible` keep the old "deadline or earlier wake" behaviour.

**Callers that genuinely want the early wake** (audited one by one, all
switched to the `_interruptible` variants — this is why the old behaviour
could not simply be replaced):

1. `WaitQueue::wait_until_timeout` / `wait_until_timeout_ns`
   (`sched/waitqueue.rs`) — a `wake_one()` before the deadline must return
   control so the condition is re-checked. Looping would degrade every
   timed condition wait into a full-timeout wait.
2. `kswapd` (`mm/kswapd.rs`) — its own comment says the ~1 s sleep "is
   interruptible via `wake_kswapd()` → `try_wake()`". Looping would delay
   reclaim by up to a full interval under memory pressure.
3. `interruptible_wait_slice` (`syscall/linux.rs`, poll/select/epoll) —
   registers as a signal waiter before parking specifically so
   `set_pending` → `try_wake` cuts the slice short.

**Found by:** extending the `BUG-SINGLE-SHOT-PARK-FABRICATES-EVENT` audit
into `sched/` itself, which was the last unaudited group of
`block_current()` sites.

### BUG-SINGLE-SHOT-PARK-FABRICATES-EVENT. `sys_irq_wait` and thread `join()` parked with a bare `block_current()` and then *assumed* their event had happened, so a spurious wake made them report an interrupt that never fired / an exit value from a thread still running — FIXED 2026-07-27

**Where:** `kernel/src/syscall/handlers.rs` — `sys_irq_wait()` (~423); and
`kernel/src/proc/thread.rs` — `join()` (~406).

**Bug.** Both were single-shot parks: register, `block_current()` once, then
treat the return as proof of the event.

* `sys_irq_wait` consumed the pending counter after waking and, when it was
  zero, **fabricated a count of 1** (`let result = if count > 0 { count }
  else { 1 };`) — literally reporting an interrupt that no ISR ever
  recorded. A driver would then go poll a device that had nothing for it.
* `join()` retrieved the exit value after waking and, when there was none,
  returned `Ok(0)` with a "shouldn't happen" warning — handing the caller a
  fake exit status for a thread that was still running, and dropping the
  registration on the floor so the *real* exit later woke nothing.

This is the residual-risk class left by
`BUG-TRYWAKE-FALSE-CONFLATES-CONTENTION`: a stale `pending_wake` token makes
the next `block_current()` return immediately. An audit of all 61
`block_current()` call sites (18 files) found every other one already inside
a condition re-check `loop {}` — the 9 in `syscall/linux.rs`, both in
`container.rs`, both `wait4` arms in `handlers.rs`, and all of the `ipc/*`
ones (rewritten during the `WaiterSet` sweep). These two were the only
exceptions.

**Second, independent defect found in the same audit.** The join wake lived
in `thread_exit_with_value()`, but that is only *one* of the ways a thread
dies. `on_thread_exit()` is the universal death hook — it is also reached
from `idt.rs` (unhandled exception kills the task), `sys_exit_group`, and
process teardown. A thread that died by any of those routes left its joiner
blocked **forever**, because nothing ever removed the `THREAD_JOIN_WAITERS`
entry or woke the waiter.

**Fix.**

* `sys_irq_wait` now loops: consume → return if non-zero → (re)register →
  park. It can only ever return a count an ISR actually recorded; the
  fabricated `1` is gone. Re-registering each iteration is an idempotent
  atomic store.
* `join()` now loops on the real condition — *its own registration*.
  `on_thread_exit` removes the entry under the `THREAD_JOIN_WAITERS` lock
  immediately before waking, so while the entry is still ours nothing has
  happened and we park again.
* The join wake **moved** from `thread_exit_with_value` into
  `on_thread_exit`, placed *before* the `THREAD_OWNERS` lookup (which
  early-returns for tasks never registered as process threads — a joiner
  must be released regardless). Every death path now releases the joiner.
  `thread_exit_with_value` still records the exit value first, so the
  ordering the joiner depends on is unchanged.
* A thread that died without recording an exit value (detached, or killed)
  now yields `Ok(0)` with an explanatory message instead of a
  "shouldn't happen" warning — that case is now expected, not anomalous.

**Regression test:** `test_blocking_join()` in `kernel/src/proc/thread.rs`
(wired into `thread::self_test()`), two phases: target records an exit value
(→ 77) and target dies *without* recording one, the crash shape (→ 0). The
target task itself asserts the joiner stays parked for 256 yields after its
registration appears — i.e. `join()` does not return while the target is
still alive. Boot-verified:
`[thread]   Blocking join (exit value recorded: true/false): OK`.

**Boot-phase note for future tests here.** The handshake is deliberately
*between the two spawned tasks*, never with the task running the self-test.
Both spawned tasks run at `DEFAULT_PRIORITY`, which outranks the boot task,
so a target gated on a flag set by the boot task **livelocks**: the target
spins `yield_now()` and the boot task is never scheduled to release it. The
first version of this test did exactly that and hung the boot (955 s
timeout, serial stops right after "Spawned task 77").

**Known remaining limitation (by design, unchanged):** `IRQ_WAIT_TASK` holds
one task per IRQ line and `THREAD_JOIN_WAITERS` one joiner per target. A
second waiter on either overwrites/`WouldBlock`s rather than queueing. Both
are documented single-waiter contracts, unlike the four IPC objects fixed in
`BUG-PIPE-SINGLE-WAITER-SLOT` (which promised multi-waiter semantics and did
not deliver them).

### BUG-PIPE-SINGLE-WAITER-SLOT. A pipe remembered only ONE blocked reader and ONE blocked writer, so a second blocker on the same pipe end was silently forgotten and never woken — FIXED 2026-07-27

**Where:** `kernel/src/ipc/pipe.rs` — the per-pipe fields `reader_waiter:
Option<TaskId>` / `writer_waiter: Option<TaskId>`, written in `read()`,
`write()`, `wait_readable()`, `read_timeout()`, `write_timeout()` and
consumed with `.take()` by the peer operation and by `close()`.

**Bug:** the fields were *single slots*, not wait queues. If task A parked on
an empty pipe (`reader_waiter = Some(A)`) and task B then parked on the same
pipe, B's assignment **overwrote** the slot. The subsequent write/close
`.take()`d only B and woke only B; **A was never woken by anything** and
parked forever. The same applied symmetrically to `writer_waiter` when two
writers blocked on a full pipe. Several waiters per end is not exotic:
`dup()` and process spawn hand the same end to multiple processes, and
`wait_readable()` (the `tee` primitive) parks on the read end alongside a
real reader.

A second, subtler defect rode along: only the *signal* exit path cleared the
waiter slot (`if pipe.writer_waiter == Some(task) { … = None }`). The
timeout path left a stale task id behind, which a later state change would
then "wake" — mis-waking whatever task had since recycled that id.

**Impact when found:** latent — every self-test and the shell plumbing used a
pipe with exactly one reader and one writer, so the slot was never contended.
It would have become real for any legitimate multi-reader/multi-writer pipe
(a worker pool where N children share one pipe end — a standard POSIX
pattern, since reads of ≤PIPE_BUF are atomic precisely so this works).

**Found by:** the BUG-DASH-CMDSUB-INTERMITTENT-HANG audit (2026-07-27), which
ruled out the lost-wakeup hypothesis on that path but surfaced this adjacent
defect.

**Fix:** both `Option<TaskId>` slots were replaced by a `WaiterSet` — a
`Vec<TaskId>` with `insert`/`remove`/`take_all` — embedded in `Pipe` and
mutated under the existing `PIPES` lock, so the documented `PIPES → SCHED`
lock order and the enqueue-inside-the-same-critical-section guarantee are
both preserved unchanged. (`sched::waitqueue::WaitQueue` was evaluated and
rejected: it owns an internal `Mutex<[u64; 32]>` that would have introduced
a *second* lock inside the `PIPES`-held critical sections, plus a new
lock-order obligation and a spin-yield-when-full failure mode.)

Three semantic changes came with it:

* **Wake-all, always.** Every state change wakes *all* waiters on the
  affected end, matching Linux (`fs/pipe.c` parks on a non-exclusive wait
  queue, so `wake_up_interruptible_sync_poll()` wakes every sleeper). This
  is required for correctness on EOF/EPIPE, which are permanent broadcast
  conditions — a waiter missed there can never be woken by anything else.
* **Deregister-first.** Every park loop now removes itself from the set at
  the top of each iteration (inside the lock), so no exit path — success,
  timeout, signal, or error — can leave a stale task id behind.
* **`close()` broadcasts.** Full closure of one end takes the whole waiter
  set of the far end and wakes all of it after dropping the table lock.

**Regression test:** `test_multi_waiter_wake()` in `kernel/src/ipc/pipe.rs`
(wired into `pipe::self_test()`). Phase 1 parks two kernel tasks on one
empty read end and does a single 2-byte write, asserting *both* readers wake
and each consumes one byte; phase 2 parks two more on a fresh empty pipe and
closes the write end, asserting both observe `Ok(0)` EOF. Under the old
single-slot code exactly one reader would have completed in each phase.

**Sweep: the same defect existed in all four blocking IPC objects.** Pipes
were only where it was noticed. `eventfd`, `stream_socket` (`socketpair`) and
`timerfd` each carried the *identical* `Option<TaskId>`-per-end
representation, with the identical overwrite and identical stale-entry-on-
timeout behaviour. All four were converted, and the representation was
factored into one shared module so they cannot drift apart again:

* **`kernel/src/ipc/waiters.rs` (new).** Home of `WaiterSet` +
  `wake_all(Vec<TaskId>)`, with the usage contract in the module docs:
  mutate the set inside the owning object's lock, `take_all()`, drop the
  lock, *then* wake (the IPC lock hierarchy always puts the object lock
  before `SCHED`); and deregister at the top of every park-loop iteration.
* **`eventfd.rs`** — `reader_waiters`/`writer_waiters`; `write`,
  `try_write`, `write_timeout`, `read`, `try_read`, `read_timeout`, `close`.
  Multi-waiter is the *design* here: `EFD_SEMAPHORE` exists precisely so N
  consumers can share one counter.
* **`stream_socket.rs`** — per-`Endpoint` sets; `send`/`recv` (+ `try_`/
  `_timeout` variants), `close`, `shutdown`. `socketpair` endpoints are
  routinely inherited by several processes.
* **`timerfd.rs`** — `reader_waiters`; `settime`, `clock_was_set`,
  `read_expirations_blocking`, `close`. The *armed* case partly survived the
  old slot (each blocked reader also arms its own expiry `hrtimer`), but the
  *disarmed* case depended entirely on the registration that `settime`
  broadcasts to, so an overwritten reader slept forever even as the timer
  fired periodically.

**Two further latent hangs found during the sweep and fixed:**

1. `stream_socket::close()` woke only the *peer* endpoint's waiters before
   removing the pair from the table. Anything still parked on the *local*
   endpoint (a caller closing the last reference out from under a blocked
   task) was then unreachable forever. It now drains all four sets.
2. `timerfd::close()` removed the table entry without waking anyone at all.
   A reader parked on a *disarmed* timerfd has no `hrtimer` of its own, so
   the final `close()` left it parked with no wake source in existence. It
   now takes the waiter set and wakes it after dropping the table lock.

**Regression tests for the sweep** (all boot-verified): `test_multi_waiter_
wake()` in `eventfd.rs` (counter wake + close wake) and in
`stream_socket.rs` (data wake + EOF wake), plus
`timerfd::self_test_blocking_multi_waiter()`.

**Boot-phase note (why the timerfd test is not in `timerfd::self_test()`).**
`ipc::timerfd::self_test()` runs in the early deterministic-init phase of
`kmain`, which is *before* `hrtimer::init()` and well before `sti()`. There
is no APIC timer ISR yet, so `hrtimer::process_expired()` is never called and
no hrtimer callback can fire — a reader that re-parks with an expiry timer
there sleeps forever. (This was confirmed empirically: a one-reader version
of the test failed at that point with `hrtimers_pending=1` after 200 ms of
elapsed monotonic time.) The blocking test therefore runs from `kmain`
immediately after interrupts are enabled and `apic::self_test()` has
confirmed the tick is live. **Any future self-test that depends on an
hrtimer callback firing must be placed after that point.**
Verified on target: `[pipe]   Multi-waiter wake (data + EOF): OK`.

### B-LIMINE-KFILE-ID. Wrong Limine kernel-file request feature-ID → boot cmdline AND kernel-file symbolization silently never worked — FIXED 2026-07-14

**Where:** `kernel/src/limine.rs`, `LimineRequest::<KernelFileResponse>::KERNEL_FILE`.

**Bug:** the request's second feature-id word was `0x31eb_5d10_c871_c930`, which
does not match Limine's `LIMINE_{KERNEL,EXECUTABLE}_FILE_REQUEST` magic — the
correct value (per `limine/limine.h`, Limine 8.7.0) is `0x31eb_5d1c_5ff2_3b69`.
Because the ID never matched, Limine never populated the response, so
`boot::kernel_cmdline()` always returned `None` and `boot::kernel_file_address()`
(used for panic/backtrace symbolization from the kernel ELF) always returned
`None`. Two silent consequences: (1) the boot command line was invisible to the
kernel — `fs::kernparam` saw an empty cmdline regardless of what the bootloader
passed, so cmdline-gated switches (e.g. `net.userspace`) could never be turned
on at runtime; (2) kernel-file-based symbolization was inert. **Fix:** corrected
the feature-id word. Verified: `cmdline: net.userspace` in `limine.conf` now
round-trips into `kernparam` and flips the cutover switch. **Repro (pre-fix):**
add any `cmdline:` to `limine.conf`; the kernel read it as empty.

### B-FUTEX-TOWAKER-LOSTWAKE. Futex timeout self-test waker could lose its wakeup (wake before waiter parked) → spurious `TimedOut` under shifted boot timing — FIXED 2026-07-14

**Where:** `kernel/src/ipc/futex.rs`, `timeout_waker_task` /
`test_timeout_woken_before_deadline`.

**Bug:** the waker calls `futex_wake` **without changing the futex word**, so
correctness depended on the waiter being parked before the wake. If the wake
fired first, the waiter re-checked its (unchanged) expected value, parked anyway,
and the earlier wake was lost — the wait then ran the full 500 ms and returned
`TimedOut`, failing the test. A fixed number of `yield_now()`s cannot guarantee
the ordering; the `net.userspace` switch-on boot shifted task-id/scheduler timing
enough to expose it (manifested as whichever timeout self-test hit the bad
interleave — channel/eventfd failures in the same window were instead
daemon-starvation, fixed separately by deferring the persistent daemon past
POST). **Fix:** the waker now retries `futex_wake` until it reports it actually
woke a waiter (bounded spin), which is deterministic regardless of interleave.

### B-EVENTFD-TOTEST-SHORTTIMEOUT. Eventfd "signaled-before-expiry" self-test used a 500 ms reader timeout + fixed-yield polling → spurious `TimedOut` (`got 18446744073709551615`) under boot-time scheduler contention — FIXED 2026-07-15

**Where:** `kernel/src/ipc/eventfd.rs`, `eventfd_timeout_reader_task` /
`test_timeout_signaled`.

**Observed:** caught during the post-serial-fix wedge-soak
(`build/hang-catches/soak-20260715-022705-iter12`): boot reached BOOT_OK but the
eventfd timeout self-test failed —
`[eventfd]   FAIL: timeout_signaled: got 18446744073709551615`
(`18446744073709551615` = `u64::MAX`, the reader's error sentinel), then
`[FATAL] Eventfd timeout self-test failed: InternalError`. Intermittent: 1 of
~13 armed boots in that soak; all other eventfd sub-tests passed.

**Root cause:** this is a *test-timing* fragility, **not** a lost-wakeup — the
scheduler's `pending_wake` protection (`sched::wake` sets `pending_wake` on a
not-yet-blocked task; `block_current` consumes it) correctly closes the
register-then-park race in `read_timeout`. Instead, the reader parked with only a
**500 ms** timeout while the main test task signaled it after a fixed
`yield + sleep_ms(5)`. During the busy boot self-test phase, transient scheduler
contention can delay the *signaling* task past the reader's 500 ms deadline, so
the reader legitimately times out (`read_timeout` → `Err(TimedOut)` → stores
`u64::MAX`) even though the eventfd signal path is correct. The main task's
fixed post-write `yield×2 + sleep_ms(5)` result check compounded the fragility
(it assumed the reader is always scheduled to completion within that window).
Same class as B-FUTEX-TOWAKER-LOSTWAKE and the channel `recv_timeout` flake.

**Fix:** (a) give the reader a generous **5 s** timeout — many orders above the
~5 ms the driver takes to signal — so the timeout can never fire under normal or
momentarily-starved scheduling (only a genuinely broken signal path fails it);
(b) replace the fixed post-write yields/sleeps with a **bounded poll loop**
(200 × `yield + sleep_ms(5)`, ~1 s cap) that waits for the reader to store its
result, so a real signal-path bug still fails deterministically in ~1 s rather
than depending on exact interleave.

### F19. rmap self-test used low fake frame addresses that collided with real CoW frames → flaky `assertion failed: is_private(frame2)` panic — FIXED 2026-06-30

**Where:** `kernel/src/mm/rmap.rs` (`self_test()`), invoked from
`kernel/src/main.rs:3288`.

**Symptom:** Intermittent boot panic `panicked at kernel\src\mm\rmap.rs:445:
assertion failed: is_private(frame2)` (also reproducible at the Test-1
`add(frame1,...)`/`count==1` assertion). The rmap self-test ran to completion on
most boots but panicked on others — pure timing/allocation flakiness, not a
deterministic failure. Surfaced while validating the container read-only-volume
work (increment 15); that change is functionally invisible to this MM path —
it merely perturbed frame-allocation timing enough to expose the latent test
bug. (A separate, also-flaky CoW-pipeline hang in the same boot run is the known
F18-family fragility of the `dash | … > file` ring-3 test and is unrelated.)

**Root cause:** The rmap is a **global** hash table keyed by physical frame
address, and `self_test()` runs *late* in boot — after the Path-Z ring-3
toolchain tests (dash pipelines, tcc, make) have done heavy CoW/fork activity
that registers thousands of **real** user frames in that global table. The test
used fixed low fake addresses (`frame1 = 0x10_0000` = 1 MiB, `frame2 = 0x20_0000`
= 2 MiB, untracked-frame probe `0xDEAD_0000`). When a real user frame happened to
sit at exactly one of those physical addresses, it already had a mapper in the
table, so the test's `add(frame2, pml4_a, virt2)` appended a *second* mapper and
`is_private(frame2)` returned false → assertion panic. Whether a real frame
landed on 0x20_0000 depended on allocation order, making it flaky.

**Fix:** Move the test frames far above any installed physical RAM (machines here
have at most a few GiB) so the global table can never hold a pre-existing entry
for them: `frame1 = 0x0F00_0000_0000` (~15 TiB), `frame2 = frame1 + 16 KiB`, and
the untracked-frame probe to `0x0F00_0001_0000`. These remain valid u64 hash keys
(the rmap does not validate physical-address width) and are impossible as real
frames, so the test is now collision-proof regardless of allocation timing. A
detailed comment records the invariant. (A fuller fix — refactoring the rmap API
to operate on an injectable test-local table instead of the global static — was
rejected as disproportionate: it would add a `&mut table` parameter to every
production rmap entry point purely for testability. Impossible-address selection
is the minimal correct fix.) The self-test still cleans up all its entries
(`frame1`/`frame2` removed before exit), so no fake entries leak into the live
table.

### F18. CoW refcount granularity mismatch (per-16 KiB-frame refcount vs per-4 KiB-PTE resolution) double-freed a still-shared frame → parent `dash` #GP in a pipeline — FIXED 2026-06-16

**Where:** `kernel/src/mm/cow.rs` (`resolve_cow_fault`, `clone_frame_group`)
and `kernel/src/mm/page_table.rs` (`clear_user_address_space`).

**Symptom:** A real `dash -c '/bin/emit | /bin/countbytes > /dash-pipe-out.txt'`
(Path Z Part 12) crashed the *parent* `dash` with a #GP at glibc
`wait4`'s errno store (`mov %eax,%fs:(%rdx)`, libc+0x110839) — but only
on the `wait4` *error* path (e.g. `-ECHILD`), which is why the
single-fork Part 11 never hit it. The faulting `%rdx` was garbage loaded
from a libc `.got` slot (the errno `R_X86_64_TPOFF64` negative TLS
offset), so `%fs:(%rdx)` was non-canonical. The `.got` 4 KiB page lived
at virt `0x6000203000`, sub-page 3 of the 16 KiB frame group based at
`0x6000200000`.

**Root cause:** CoW refcounting is **per-16 KiB frame** (the buddy
allocator's unit), but CoW *sharing/resolution* is tracked **per-4 KiB
PTE** (each 16 KiB frame maps as 4 consecutive PTEs). The ELF loader
packs a read-only segment tail and a writable segment head into one
16 KiB frame, so a group can hold a read-only *shared* sub-PTE (no COW
bit) next to a writable *CoW* sub-PTE — both pointing into the same
frame. Three operations used **inconsistent** rules for "the group's
reference to the frame":
- `clone_frame_group` incremented the refcount once, keyed on the *first
  present* sibling.
- `resolve_cow_fault` decremented once per resolve event whenever *any*
  CoW sibling was copied out — **even though a read-only shared sibling
  still referenced the old frame**.
- `clear_user_address_space` freed once per group, keyed on *only the
  base (sub-page 0)* PTE.

So a forked child that wrote the writable sub-PTE resolved it to a
private copy and decremented the old frame, *while still mapping the old
frame via the read-only sub-PTE*. At teardown the child's base PTE still
pointed at the old frame → it freed it **again** (double-decrement). Two
such children drove the parent-shared frame's refcount to 0; the freed
frame was reused (filled with a child's exec image), corrupting the
parent's `.got` errno slot → garbage `%rdx` → #GP.

**Fix:** Make all three operations agree on one invariant — *each address
space holds exactly one refcount on each **distinct** 16 KiB frame its
group's sub-PTEs reference*:
- `resolve_cow_fault` now drops the old frame's reference (ref_dec + rmap
  remove) **only if, after the copy loop, no sub-PTE of the group still
  points into the old frame**. A read-only shared sibling keeps the
  reference alive; the new private frame is registered in rmap
  unconditionally.
- `clone_frame_group` increments the refcount (and adds rmap) once **per
  distinct frame** found among the group's present siblings (handles a
  parent that had already partially resolved a group before forking
  again).
- `clear_user_address_space` inspects **all four** sub-PTEs of each group
  and frees each **distinct** frame exactly once (was: only the base
  PTE), so copied-out private frames are no longer leaked and refcounts
  stay symmetric with resolve/clone. (The refcount-aware `free_frame`
  already only returns a frame to the allocator at its last reference.)

**Verification:** Part 12 boot self-test
`proc::spawn::self_test_linux_real_glibc_shell_pipe` now passes (parent
`dash` exits 0, `/dash-pipe-out.txt` == `n=16\n`).

### F17. fd-bearing resources were closed at *reap* (`destroy`) instead of at *exit* (zombie) → `cmd1 | cmd2` pipeline deadlock — FIXED 2026-06-16

**Where:** `kernel/src/proc/pcb.rs` — new `exit_close_fds(pid)` + extracted
`close_initial_fds()`; `kernel/src/proc/thread.rs` — `on_thread_exit` calls
`pcb::exit_close_fds(pid)` at the zombie transition;
`kernel/src/proc/pcb.rs::destroy_process_resources` now just calls
`cleanup_handles` + `close_initial_fds` for the force-kill / never-zombied
path (the slices are already empty on the normal exit path).

**Symptom:** A real glibc `cmd1 | cmd2` pipeline (`/bin/pipe`: `pipe`→`fork`;
child `dup2`s the write end onto fd 1 and `execl`s `/bin/emit`; parent closes
the write end, `read`s the pipe to EOF, then `waitpid`s the child) **hung
forever** — `self_test_linux_real_glibc_pipe` reported "process did not exit
within N yields (state=Running)" regardless of the yield budget (a 4×
budget bump changed nothing — the tell that it was a deadlock, not
under-budgeting).

**Root cause:** A blocked pipe reader only gets EOF (`read()`→0) when the
*last* write end closes. The child's exec'd image inherited a copy of the
pipe write end; that fd's kernel resource was only released by
`destroy_process_resources`, which ran when the **parent reaped** the child
via `wait4`. But the parent could not reach `waitpid()` until its `read()`
returned EOF. EOF ⟸ child's write end closed ⟸ child reaped ⟸ parent past
`read()` ⟸ EOF. Circular wait → deadlock.

**Fix:** Close every fd-bearing kernel resource (all `ipc_handles` + any
unclaimed initial fds) the moment a process **exits** (becomes a zombie),
not when its parent reaps it — matching Linux's `exit_files()` in `do_exit`.
`exit_close_fds` `core::mem::take`s the two lists out of the PCB under the
table lock, drops the lock, then dispatches `cleanup_handles` +
`close_initial_fds`. Idempotent: the reap-time teardown finds the lists
already drained, so no double-close and no leak; the force-kill path (where
a process is destroyed without ever zombying) still closes everything.

**Validation:** `self_test_linux_real_glibc_pipe` now passes — the parent
wakes from `read()` the instant the child zombies, prints
`SLATE_GLIBC_PIPE_OK n=16 body=SLATE_PIPE_BODY\n` (46 bytes captured ==
expected) and `exit(29)`; boot test PASSED. This is a general correctness
fix: it affects every pipe/socket EOF-on-last-writer-exit, not just the
test. It is also the standing semantics any real shell relies on.

### F16. `on_thread_exit_hook` dereferenced user pointers unconditionally → kernel page-fault panic when thread cleanup ran cross-address-space — FIXED 2026-06-16

**Where:** `kernel/src/proc/thread_clone.rs` — `on_thread_exit_hook(task_id)`.

**Symptom:** `PANIC` — page fault in `read_user` reached via
`fetch_robust_entry ← exit_robust_list ← on_thread_exit_hook`, with CR2 in a
glibc-mmap user range, when a boot self-test reaped a real glibc process
(e.g. the Part 7 pipe test) by calling `thread::on_thread_exit(task_id)` from
**task 0's (boot) address space** rather than the dying process's.

**Root cause:** The exit hook walked PI-owned futexes, the glibc robust
list, and zeroed `clear_child_tid` — all of which dereference *user* virtual
addresses valid only in the dying process's address space. When the hook
runs from a different active CR3 (cross-AS reap), those addresses point into
the wrong (or unmapped) address space → faulting kernel read → panic.

**Fix:** AS-active guard. The hook computes
`as_active = page_table::active_pml4_phys() == pcb::get_pml4(owner_process(task_id))`
and runs the user-memory operations (PI-futex walk, robust-list walk,
`clear_child_tid` zero-write + `futex_wake`) **only when `as_active`**. The
in-kernel bookkeeping removals (`ROBUST_LIST` / `RSEQ` / `CLEAR_CHILD_TID`
map entries) always run regardless. When not AS-active the hook skips the
user dereferences and returns after the in-kernel cleanup — correct, because
the futex-wake/ctid-clear only matter to a live address space, and a process
being reaped from outside its own AS has no threads left to wake.

**Validation:** the Part 7 pipe boot test no longer panics in the robust-list
walk; boot test PASSED.

### F15. Sleep-queue slot leak: an expired entry was only freed when `try_wake` returned `true`, so tasks woken early / destroyed before their deadline leaked a slot permanently — daemons then busy-spun and starved low-priority work — FIXED 2026-06-14

**Where:** `kernel/src/sched/mod.rs` — `process_sleep_wakeups()` and the new
`wake_expired_sleeper()` helper; the fixed-size `SLEEP_QUEUE` (`MAX_SLEEPERS`
= 256) and the `sleep_until_tick()` busy-spin fallback.

**Symptom:** Surfaced while adding a `--bench` mode to `scripts/boot-test.sh`
(which waits for the deferred `BENCH_OK` instead of stopping at `BOOT_OK`).
During the post-boot benchmark phase the serial log filled with **688**
`[sched] WARNING: sleep queue full, task <N> falling back to spin` lines —
tasks 103 (kswapd) and 104 (the workqueue worker), both long-lived daemons
that sleep between work, could no longer register a sleep, so they fell back
to the `yield_now()` busy-spin loop in `sleep_until_tick()`. That pinned a CPU
and starved the low-priority deferred-benchmark task. The default boot test
never saw this because it kills QEMU at `BOOT_OK`, before the daemons have
looped enough to exhaust the queue.

**Root cause:** `process_sleep_wakeups()` (timer-ISR tick handler) cleared an
expired slot only when `try_wake(task_id)` returned `true`. But `try_wake`
returns `false` in two fundamentally different situations:
1. **Lock contended** (`SCHED.try_lock()` failed) — transient; retrying next
   tick is correct.
2. **Task not `Blocked` / no longer in the table** — terminal. A task that
   slept and was then woken early through another path (channel/futex/eventfd
   wake), or that was destroyed before its deadline, is no longer `Blocked`,
   so `try_wake` can *never* succeed for that slot again.
The code conflated the two and kept the slot in both cases. In the terminal
case the slot was retained forever — a permanent leak. As short-lived
boot/self-test/benchmark tasks slept-then-exited, slots leaked one by one
until all 256 were gone, after which every subsequent sleeper busy-spun.

**Fix:** Split the two failure modes with a dedicated `wake_expired_sleeper()`
that returns `SleeperWake::{Release, Retry}`. It acquires the scheduler lock
itself: on `try_lock` failure it returns `Retry` (keep the slot — genuine
contention); otherwise it inspects the task and returns `Release` in **all**
non-contention cases — task still `Blocked` (wake it, as before), task present
but already awake (record `pending_wake`, release), or task gone (release).
`process_sleep_wakeups()` now clears the slot whenever it gets `Release`, so an
expired slot is reclaimed at its deadline at the latest, bounding occupancy to
"tasks with un-expired deadlines" instead of leaking permanently. Verified by
re-running `scripts/boot-test.sh --bench --no-build`: the
`sleep queue full` warning count dropped from **688 to 0**, with the benchmark
numbers up to `context_switch_rt` captured cleanly.

**Residual (separate, pre-existing):** `BENCH_OK` is still not reached — the
deferred benchmark suite livelocks later, in `bench_pick_next` (logged
separately under Active Bugs as the "deferred benchmark suite hangs after
`context_switch`" item). That hang reproduced identically *before* this fix
(when it was masked by the spin-starvation) and *after* it (0 spin warnings),
confirming it is independent of the slot leak.

### F14. `arch_prctl(ARCH_SET_GS)` wrote `KERNEL_GS_BASE` (Linux convention) but Slate's entry stub uses the inverted GS convention → first syscall after SET_GS faulted on per-CPU access — FIXED 2026-06-14

**Where:** `kernel/src/syscall/linux.rs` `sys_arch_prctl` (ARCH_SET_GS /
ARCH_GET_GS arms); the userspace `%gs`-base context-switch restore in
`kernel/src/sched/mod.rs` (both switch sites); the `execve` `%gs` reset in
`kernel/src/proc/spawn.rs`.

**Symptom:** Latent until exercised. The new two-process `%gs`-base
context-switch regression test (`self_test_linux_gs_tls_switch`) reliably
triggered it: a ring-3 process that issued `arch_prctl(ARCH_SET_GS, sentinel)`
and then made *any* further syscall took an unrecoverable kernel `#PF` writing
to `sentinel + 8` — i.e. the syscall entry stub's `mov gs:[8], rsp` was
dereferencing the user's `%gs` sentinel as if it were the per-CPU base. With
no real ring-3 caller ever issuing ARCH_SET_GS before this test, the bug had
shipped undetected.

**Root cause — two self-consistent GS conventions, mixed:**
- **Linux convention:** syscall handlers run with the per-CPU pointer *active*
  in `GS_BASE` (one `SWAPGS` at entry, one at exit) and the userspace value
  parked in `KERNEL_GS_BASE`. So Linux's `ARCH_SET_GS` writes `KERNEL_GS_BASE`.
- **Slate's actual entry stub** (`kernel/src/syscall/entry.rs`) does a *second*
  `SWAPGS` back before calling the Rust handler, so a handler runs with the
  userspace `%gs` base *active* in `IA32_GS_BASE` and the per-CPU pointer
  resting in `KERNEL_GS_BASE`. Phase 4 swaps again for per-CPU stack access on
  the way out. Interrupts never `SWAPGS` at all. The invariant is therefore
  "**`KERNEL_GS_BASE` always holds the per-CPU pointer while in the kernel**,"
  and the userspace `%gs` base is simply the active `IA32_GS_BASE` — fully
  symmetric to `%fs`/`IA32_FS_BASE`.

  The pre-existing `ARCH_SET_GS` was copied from the *Linux* convention
  (writing `KERNEL_GS_BASE`), which under Slate's stub clobbers the per-CPU
  pointer mid-handler; phase 4's `mov gs:[8], …` (after its `SWAPGS` brings the
  now-corrupted `KERNEL_GS_BASE` into the active slot) then faults.

  A first attempt at the context-switch restore made the same wrong assumption
  in the other direction — it tried to fall back to a "live per-CPU base" read
  from `IA32_GS_BASE` when a task had no custom `%gs`. But inside a syscall
  handler `IA32_GS_BASE` holds the *user's* base (0 for a never-set task), so
  that read yielded 0 and the next `SWAPGS` loaded `GS_BASE = 0`, faulting per-CPU
  access on the *first* ring-3 process spawned.

**Fix:** Treat the userspace `%gs` base exactly like `%fs` — it is the active
`IA32_GS_BASE`. `ARCH_SET_GS`/`ARCH_GET_GS` now write/read `IA32_GS_BASE`
(0xC000_0101), not `KERNEL_GS_BASE`; the scheduler restores
`wrmsr(IA32_GS_BASE, task.gs_base)` on switch-in for user tasks (0 = no custom
`%gs`, the default — correct to restore directly); `execve` resets
`IA32_GS_BASE = 0`. `KERNEL_GS_BASE` is now written in exactly one place
(`syscall::entry::init`, the per-CPU pointer) and never touched again, making
the invariant trivially true. The TD4 `arch_prctl` GS validation self-test was
updated to bracket `IA32_GS_BASE` instead of `KERNEL_GS_BASE`. Verified: build
+ clippy (0 errors) + boot-test green; both the `%fs` and `%gs` two-process
context-switch regression tests print OK and there are no panics.

**Lesson:** When two layers each encode a CPU-state convention (the asm entry
stub vs. the syscall handler), they must agree explicitly. The FS/GS-base
handling is the canonical example; both are now documented as "active-register,
symmetric to %fs" on `cpu::IA32_GS_BASE`, `Task::gs_base`, and the
`sys_arch_prctl` const doc.

### F13. Userspace `%fs` (TLS) base and `%gs` base were not saved/restored per task across context switches — FIXED 2026-06-14

**Where:** `kernel/src/sched/mod.rs` context-switch path (both switch sites);
`kernel/src/sched/task.rs` (`fs_base`/`gs_base` fields);
`kernel/src/syscall/linux.rs` `sys_arch_prctl`; `kernel/src/proc/{fork,
thread_clone,spawn}.rs`.

**Symptom:** Latent for single-process workloads; fatal for any multi-process
glibc workload (a real toolchain: gcc/ld/make/bash). `IA32_FS_BASE` is glibc's
thread-local-storage pointer (`%fs` base) and is a global CPU register *not*
part of the saved GP `Context`. With two concurrent glibc processes, a context
switch left the incoming process running on the outgoing process's TLS pointer
— silently corrupting `errno`, the stack-protector canary, and every `__thread`
variable. The `%gs` base (see F14) is the sibling register with the same flaw.

**Root cause:** The scheduler swapped CR3, FPU state, and the GP register
`Context` on a switch, but never the per-thread segment-base MSRs. `CR4.FSGSBASE`
is off, so userspace can only change these via `arch_prctl`/`CLONE_SETTLS`,
making a kernel-stored per-task field authoritative.

**Fix:** Added authoritative per-`Task` `fs_base`/`gs_base` fields, restored on
switch-in for user tasks (`pml4_phys != 0`), kept in sync at
`arch_prctl(ARCH_SET_FS/SET_GS)`, inherited across `fork`/`clone`, and reset on
`execve`. Two two-process ring-3 regression tests
(`self_test_linux_fs_tls_switch`, `self_test_linux_gs_tls_switch`) install
distinct sentinel bases in concurrent processes and assert each survives
cooperative yields; both print OK at boot. (See F14 for the `%gs`-specific
convention subtlety that the GS half of this work uncovered.)

### F12. ALSA PCM `hw_params` leaked a mixer slot under concurrent calls on a shared fd — FIXED 2026-06-13

**Where:** `kernel/src/ipc/alsa_pcm.rs` `hw_params` (the slot-reservation
re-acquire path, ~lines 376-410).

**Symptom:** None observed yet (latent). Two concurrent `SNDRV_PCM_IOCTL_HW_PARAMS`
ioctls on the *same* PCM fd — reachable when a fd is shared across threads or
inherited across `fork()` — could permanently leak one `audio_mixer` stream
slot. Mixer slots are a finite resource, so repeated occurrences would
eventually exhaust them and make `open_stream` fail with `WouldBlock` for all
clients.

**Root cause:** A TOCTOU window in the leaf-lock dance. `hw_params` read
`need_stream = pcm.mixer_stream.is_none()` under the table lock, dropped the
lock to call `audio_mixer::open_stream()` (which must not run under the table
lock), then re-acquired the lock and did `pcm.mixer_stream = Some(sid)`
**unconditionally**. Two racing calls both observed `mixer_stream == None`, both
opened a slot, and the one that re-acquired the lock second overwrote the
first's stored `StreamId` — orphaning it (it was never `close_stream`d; the
instance's eventual `close` frees only the surviving slot).

**Fix:** On re-acquire, only store the freshly-opened slot if `mixer_stream` is
still `None`; otherwise treat it as redundant, keep the existing slot, and free
the redundant one with `audio_mixer::close_stream` *after* dropping the table
lock (preserving the documented leaf-lock invariant — no mixer call under the
table lock). Added a single-threaded idempotency assertion to the self-test
(a repeat `hw_params` stays `SETUP` with unchanged params, exercising the
`need_stream == false` reuse branch).

### F11. hrtimer self-test Test 2 raced the APIC timer ISR → intermittent boot panic — FIXED 2026-06-12

**Where:** `kernel/src/hrtimer.rs` self-test Test 2 (~lines 475-496).

**Symptom:** Intermittent boot panic at `hrtimer.rs:488`
`"Timer with 0 delay didn't fire on process_expired()"`. The panic blocked
the boot gate for any batch whose validation run happened to lose the race,
even though the code under test was correct.

**Root cause:** The self-test runs with interrupts ENABLED. It scheduled a
0-delay timer and then called `process_expired()` manually, expecting to
drain it. But the periodic APIC timer ISR also calls `process_expired()`;
when the ISR fired in the window between `schedule_ns` and the manual
`process_expired()`, the ISR drained the 0-delay timer first, so the manual
call returned `n == 0` and the `assert!(n >= 1, ...)` panicked.

**Fix:** Wrap the `schedule_ns(0, ...)` + `process_expired()` pair in
`crate::cpu::without_interrupts(|| { ... })` so the manual drain is
deterministic — the ISR cannot steal the timer in between. Test-only
correctness fix; the hrtimer subsystem itself was already correct.

### F10. Boot-stack overflow from monolithic translation self-test silently corrupted `.bss` (FPU_STRATEGY) → futex-test `#UD` — FIXED 2026-06-12

**Where:** `kernel/src/main.rs` boot stack (`KERNEL_BOOT_STACK`, was 512 KiB)
vs. `kernel/src/syscall/linux.rs::self_test()` (a single ~1.4 MB monolithic
function). Crash surfaced in `kernel/src/sched/context.rs::switch_context`
reading `sched::context::FPU_STRATEGY`.

**Symptom:** Boot reached `[syscall/linux] Translation self-test PASSED`,
then the very next subsystem — `ipc::futex::self_test()` — spawned task 36
("futex-test") and the first context switch faulted:
`EXCEPTION: Invalid Opcode (#UD) at 0xffffffff81133b0e`, instruction bytes
`49 0f ae 20` (= `xsave64 [r8]`), then `FATAL: Unrecoverable kernel #UD`.
The kernel never reached `BOOT_OK`, so boot-test could not pass. Appeared
only after the batch-536 ABI change (a translator-only `sys_fallocate`
gate not even exercised by the futex test) — a classic layout-shift
heisenbug. Reproduced deterministically with batch 536 applied; passed
deterministically with it stashed.

**Root cause:** Boot-stack overflow. `switch_context` dispatches the FPU
save on the global `FPU_STRATEGY` byte (0=FXSAVE, 1=XSAVE, 2=XSAVEOPT).
Boot init selected **FXSAVE** (QEMU CPU reports no XSAVE; serial line 84:
`strategy=FXSAVE`), yet the crashing switch executed the **XSAVE64**
branch → `FPU_STRATEGY` had been corrupted 0→1. The corruptor: the
monolithic `syscall::linux::self_test()` runs directly on the boot stack
and, in the unoptimized debug build (`opt-level=0`, no stack-slot
coloring), its frame is the *sum* of every per-batch block's locals —
disassembly of the prologue showed a ~480 KiB frame (`sub r11, 0x75000` +
probe loop + `sub rsp, 0x900`). With the 512 KiB boot stack (no guard
page) minus `kernel_main`'s own frame, batch 536's extra locals tipped the
frame past the stack bottom; the prologue's page-probe / frame writes
scribbled the adjacent `.bss`, flipping the `FPU_STRATEGY` byte to 1. The
self-test still completed (it never re-reads that byte), printed PASSED,
and returned — the poison only bit later when the futex context switch
trusted the corrupted strategy and ran `xsave64` on a CPU without
`CR4.OSXSAVE` → `#UD`. The boot stack having **no guard page** is what made
the overflow silent instead of a clean fault (same silent-`.bss`/page-table
class noted in the `KERNEL_BOOT_STACK` doc comment for the original Limine
stack).

**Fix (`kernel/src/main.rs`):**
1. Enlarged `KERNEL_BOOT_STACK_SIZE` 512 KiB → **2 MiB** so the boot-time
   self-tests fit with generous headroom (~1000+ ABI batches of runway).
2. Added a **64 KiB bottom redzone canary** (`BOOT_STACK_REDZONE`,
   `BOOT_STACK_CANARY = 0xC7`): `init_boot_stack_canary()` fills it early in
   `kernel_main` (RSP near top), `check_boot_stack_canary()` (called right
   after `syscall::linux::self_test()`) volatile-scans it and FATAL-halts
   with a clear "boot stack overflow detected" message if clobbered. The
   unoptimized stack-probe prologue writes a zero to every 4 KiB page it
   descends through, so any frame that reaches the redzone is guaranteed to
   trip the canary — converting future silent overflows into clear
   diagnostics before they can corrupt the `.bss` below the stack.

**Proper long-term fix (tracked as TD4):** the real smell is the monolithic
~1.4 MB `self_test()` with an unbounded per-batch frame. It should be split
into many small `#[inline(never)]` sub-functions so no single frame is
large. Deferred because the function is one giant 4-space enclosing block
(~39 k lines, opens early / closes at line 75298) and a hand-split risks
silently mis-scoping shared locals; the 2 MiB stack + canary make the
system correct and self-diagnosing in the meantime.

**Verification:** boot-test with batch 536 applied now reaches `BOOT_OK`
in 26s (was deterministic `#UD` FATAL before `BOOT_OK`), with serial
running through to the `user>` shell prompt; the redzone canary scan runs
clean (no "boot stack overflow detected"), `[syscall/linux] Translation
self-test PASSED`, and the futex self-test that previously faulted now
completes normally. (One of the validation runs hit the pre-existing
intermittent OOM-self-test truncation tracked as W1 — unrelated to this
fix; the immediate re-run was clean.)

### F8. quota self-test Test 5: wrong inode expectation (test bug, not production) — FIXED 2026-06-10

**Where:** `kernel/src/fs/quota.rs` — `self_test()` Test 5.

**Symptom:** Boot serial printed a non-fatal ERROR "expected Allowed at
limit, got SoftWarning" from Test 5.

**Root cause:** A *test* bug, not a production-code bug. Test 2 sets the
test user's limits to `soft_inodes = 100, hard_inodes = 200`. Test 5
then set usage to 199 inodes and expected `check_create()` to return
`Allowed`, with a comment reasoning only about the hard limit ("→ 200,
equals hard, should be allowed"). It ignored that 199 inodes is already
far over the soft limit of 100, so `check_inodes()` correctly returns
`SoftWarning` (199+1 = 200 > soft 100; grace not yet enforced). The
production check path is correct and symmetric with `check_bytes()`
(both use `new_total > limit`): there is no inode-vs-byte off-by-one.

(Initially mis-logged as Active bug A1 — a supposed production off-by-one
in the inode soft-limit boundary. That was wrong; corrected on the same
day after reading the limit setup.)

**Fix:** Rewrote Test 5 to exercise all three quota bands the way Tests
2-4 do for bytes — under-soft (50 inodes → Allowed), over-soft within
grace (150 → SoftWarning), and at the hard limit (200 → Denied) — so it
validates real inode-quota semantics instead of asserting a value the
code never produces.

**Verification:** boot-test — quota self-test reaches "[quota]   inode
limit OK" with no ERROR.

### F9. FS interceptor deny handlers fail open for trailing-slash prefixes — FIXED 2026-06-10

**Where:** `kernel/src/fs/intercept.rs` — `pre_check()` interceptor
match filter.

**Symptom:** Boot serial printed non-fatal "[intercept]   ERROR: deny
handler allowed". A `Deny` interceptor registered for `/protected/` did
not block a write to `/protected/secret.txt` — it failed *open*.

**Root cause:** The match filter used
`path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/')`,
but interceptors are registered with a **trailing-slash** prefix
(`/protected/`). With the slash included, `get(prefix.len())` looks at
the byte *after* the slash, so the check only matched double-slash paths
(`/protected//x`). Real children like `/protected/secret.txt` never
matched, so the deny handler was never invoked and the operation was
allowed. (Same idiom bug as F-class integrity.rs fix in commit
`22a8098f`; see TD3 for the broader audit.)

**Fix:** Extracted `path_matches_prefix(path, prefix)` which normalises
away a single trailing slash (`strip_suffix('/')`) before applying the
canonical component-boundary check, so it is correct whether or not the
registrant supplied a trailing slash, and also matches the protected
directory node itself (`/protected`). Added boundary regression
assertions to Test 3: `/protectedX/file.txt` must NOT match (no prefix-
string leak) and `/protected` (the dir itself) must match.

**Verification:** boot-test — "[intercept]   deny handler with path
prefix OK" and "[intercept] Self-test passed (10 tests)" with serial
showing DENIED on both `/protected/secret.txt` and `/protected` and no
denial of `/protectedX/...`.

### F1. RCU self-test occasionally hangs at boot (intermittent) — FIXED 2026-06-07

**Where:** `kernel/src/rcu.rs` — `call()`, `process_callbacks()`,
`stats()` and (defense-in-depth) `synchronize()`.

**Root cause:** The `CALLBACKS` spinlock was acquired both from
direct callers (boot path → `rcu::call`, `rcu::stats`,
`rcu::synchronize` → `process_callbacks`) AND from `rcu::tick()`
running in softirq context.  Softirqs dispatch with interrupts
re-enabled on the same CPU.  If a timer ISR fired while a direct
caller held the lock, the softirq's `process_callbacks()` re-entered
the same critical section on the same CPU and deadlocked the
spin::Mutex.  The hang manifested between
`[rcu]   Quiescent state: OK` and `[rcu]   Callback registration: OK`
(i.e. inside `rcu::call`) because that's the first lock acquisition
after the periodic softirq starts running.

**Diagnosed by:** Running boot-test.sh 10× — observed 2 hangs, both
with the serial log truncated at exactly the same point (after
"Quiescent state" probe, before "Callback registration").  This
showed the hang was in `call()`, not `synchronize()` as the original
hypothesis suggested.

**Fix:** Wrap every `CALLBACKS.lock()` site in
`crate::cpu::without_interrupts(...)` so the lock cannot be acquired
from a path that is interruptible.  Additionally, in `synchronize()`,
explicitly bump the calling CPU's own QS counter after snapshotting
(the caller cannot itself be in a read-side critical section by RCU
invariant), and add a million-iteration safety cap with diagnostic
print so any future grace-period failure surfaces a warning instead
of a silent hang.  Added finer-grained "[rcu]   Synchronize: pre/post"
self-test probes to localize any future regression.

**Verification:** 20/20 consecutive boot tests pass after the fix
(previously 2/10 hung).

### F2. Watchdog self-test heartbeat-increment assertion race — FIXED 2026-06-07

**Where:** `kernel/src/watchdog.rs` — `self_test()` test 1.

**Root cause:** The test does
`before = HEARTBEATS[cpu].load(); heartbeat(); after = HEARTBEATS[cpu].load();`
and asserts `after == before + 1`.  But the APIC timer ISR also calls
`watchdog::heartbeat()` on every tick (via `apic.rs`), so a timer
interrupt landing inside the before→after window can cause the
counter to advance twice, tripping the assertion.  Observed once on
2026-06-07: panic with `left: 368, right: 367`.

**Fix:** Wrap test 1's load/heartbeat/load sequence in
`crate::cpu::without_interrupts(...)`.

**Verification:** 20/20 consecutive boot tests pass after the fix.

### F3. Softirq self-test races APIC timer ISR — FIXED 2026-06-07

**Where:** `kernel/src/softirq.rs` — `self_test()` tests 2, 3, and 4.

**Root cause:** The self-test runs after `[boot] Interrupts enabled —
preemptive scheduling active`, so the APIC timer ISR fires
asynchronously throughout the test.  The ISR's path calls
`process_pending()` on the same CPU, which mutates `TOTAL_RUNS`,
`TOTAL_HANDLERS`, `IN_SOFTIRQ`, and `PENDING`.  Three races:

  * Test 2 (no-op fast path): an ISR firing between
    `process_pending()` returning and `TOTAL_RUNS.load()` bumps the
    counter and trips `runs_after != runs_before`.
  * Test 3 (dispatch + clear): an ISR firing between `raise()` and
    the test's own `process_pending()` drains TIMER_SOFTIRQ first;
    the test's call then runs no handler and trips
    `handlers_after <= handlers_before`.
  * Test 4 (re-entry guard): after the test clears
    `IN_SOFTIRQ[cpu] = false`, an ISR firing before the
    `still_pending` load runs a real `process_pending()`, consumes
    TIMER_SOFTIRQ, and trips "bits were consumed despite re-entry
    guard".  Observed once on 2026-06-07 during the post-RCU-fix
    soak (build/serial-test.txt at 11:44).

**Fix:** Wrap each of tests 2, 3, and 4 in
`crate::cpu::without_interrupts(...)`.  In test 4, also sample
`PENDING` *before* clearing `IN_SOFTIRQ` so the semantic ordering
("did the guarded call consume bits?") is preserved.  `process_pending`
internally toggles IF (STI→handlers→CLI); `without_interrupts` saves
and restores the outer IF state, so the boot path's interrupt state
post-test is unchanged.  Test 1 already had its own CLI/STI window
and didn't need changes.

**Verification:** Boot test passes cleanly with `softirq` self-test
showing all four sub-tests OK and `Self-test PASSED`.  Post-fix
30-run soak: 29/30 pass with zero softirq self-test failures (the
single failure was in `frag_history` test 6 — see F4 below).

### F5. `frame::ALLOCATOR` lock uniformly IRQ-safe — FIXED 2026-06-07

**Where:** `kernel/src/mm/frame.rs` — all 13 remaining `allocator.lock()`
acquisition sites outside `pcpu_refill`/`pcpu_drain` (which are
already called with IRQs off) and `try_stats()` (panic-only).

**Why this was technical debt (was TD1):** F4 made `stats()`
IRQ-safe but left `alloc_*`, `free_*`, `is_allocator_owned`,
`refcount`, `ref_inc`, `ref_dec`, and `validate_free_lists` taking
the lock without wrapping in `without_interrupts`.  No
currently-registered softirq path took the allocator lock (audited
2026-06-07), so there was no exploitable deadlock — but the next
softirq subsystem that touched the allocator (kswapd periodic
reclaim, RCU-deferred page free, memory-pressure tick) would have
silently re-opened the same race that F4 closed.

**Fix:** Wrap each acquisition site in
`crate::cpu::without_interrupts(...)` at the call site, matching
the F1/F3/F4/workqueue pattern.  The multi-attempt `alloc_order_inner`
and `alloc_order_constrained_inner` paths use a per-attempt
without_interrupts so IRQs are re-enabled between attempts (so
reclaim/compact/OOM can run normally and wake other tasks).  Did
NOT wrap `pcpu_refill` / `pcpu_drain` — their callers already run
with IRQs disabled and the function-level comments document this
invariant.  Used inline wraps rather than a helper because the
sites have varied shape (KernelResult returns, multi-attempt retry
loops, value vs Option returns) — a `with_allocator` helper would
have required `FnOnce(&mut BuddyAllocator) -> R` plumbing at every
site, which is more code churn than the wraps themselves.

**Verification:** Post-fix 30/30 boot tests pass.  Zero allocator-lock
hangs observed across this soak.

### F4. frag_history self-test test 6 hangs in sample() loop — FIXED 2026-06-07

**Where:** `kernel/src/mm/frag_history.rs` — `self_test()` test 6
("Ring buffer wraps correctly"), inside the
`for _ in 0..HISTORY_SIZE + 5 { sample(); }` loop.

**Root cause (hypothesis, verified by soak):** `sample()` calls
`mm::frame::stats()` on every iteration, which acquires
`frame::ALLOCATOR.lock()`.  The boot path runs with interrupts
enabled, so an APIC timer ISR could fire on the same CPU while the
lock was held.  Per a softirq-handler audit, no currently-registered
softirq path takes `ALLOCATOR.lock`, so a clean dead-lock chain
wasn't conclusively proven — but the empirical data (hang exactly
in this 37-iteration tight loop over a lock-acquiring call) plus
the cure (see Fix) make this the most likely explanation.  A
plausible alternate path: any future softirq subsystem (kswapd
periodic reclaim, RCU-deferred page free, memory-pressure tick)
that touched the allocator would have re-introduced the race.

**Diagnosed by:** Post-F3 30-run soak showed `[frag_history]
Trend: OK (Stable)` as the last serial line of one failure
(`build/soak-hang-run18.txt`).  Bisected the hang window to the
test 6 sample-loop.

**Fix:** Made `frame::stats()` itself IRQ-safe by wrapping the
`ALLOCATOR.lock()` acquisition in `crate::cpu::without_interrupts(...)`.
The companion `try_stats()` (panic-handler variant) already used
`try_lock()` for the same family of reasons; this brings the
regular `stats()` to parity.  Hardening — eliminates an entire
class of same-CPU IRQ-vs-main deadlocks on the buddy allocator
lock without measurable performance cost (CLI/STI on a stats read
that already serializes on a spinlock is negligible).

**Verification:** Post-fix 30/30 boot tests pass; zero recurrence
of the frag_history hang AND zero recurrence of Active Bugs #1
(accounting) and #2 (invariant) over those same 30 runs.

---

### TD-ARCHIVE-WRITER-NAMES-ARE-STRING-NOT-BYTES. ar/rar/7z member names were still `String` — LOGGED 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/ar.rs` (`ArEntry::name`), `kernel/src/fs/rar.rs`
(`RarEntry::name`), `kernel/src/fs/sevenz.rs` (`SevenZEntry::name`,
`FileInfo::name`).  The narrowing point was
`kernel/src/fs/archive.rs::name_for_string_writer`, now deleted.

**What it was:** as part of `D-VFS-PATHS-ARE-STR-NOT-BYTES`, `fs::tar`,
`fs::cpio`, `fs::zip` and the unified `fs::archive` layer moved to
`fs::path::PathBuf` (raw bytes) member names.  These three format modules
still modelled theirs as `String`.  On the *read* path that is harmless only
if the parser did not already mangle the name — `archive::list_*` widens
`String → PathBuf` losslessly, but it cannot undo a lossy decode.  On the
*write* path a name that is not valid UTF-8 could not be handed to those
writers at all, so `archive::create` rejected it with
`KernelError::InvalidArgument` — an honest failure, but a capability gap:
every one of these formats stores names as raw bytes on disk.

**As predicted, each parser carried its own variant of the mangling bug**, and
converting them uncovered both: see `B-RAR-DROPS-NON-UTF8-MEMBER-NAMES`
(non-UTF-8 name → `""`, member lost or colliding) and
`B-7Z-COLLAPSES-UNPAIRED-SURROGATES-IN-MEMBER-NAMES` (`from_utf16_lossy`
merged distinct Windows names onto one, so extraction overwrote a file).
That is now four format parsers (cpio, zip, rar, 7z) that each invented a
name because they had nowhere to put non-UTF-8 bytes — the recurring shape,
not a coincidence.

**Fix:** all three entry types carry `PathBuf` end to end.  Specifically:

- **ar** — the parser trims the header's space padding and strips the single
  `/` terminator (which is what lets a name *ending in a space* survive), and
  resolves `/<offset>` against the GNU `//` long-name table with an
  empty-name guard.  Because `ar` has no escape mechanism at all, `mkar` runs
  a pre-pass (`check_member_name`) rejecting the three shapes it genuinely
  cannot encode: an empty name, a name starting with `/` (which would be read
  back as a long-name reference or the symbol table) and one containing the
  `/\n` sequence that terminates a long-name-table record.  `create_ar` keeps
  its `InvalidArgument` for those — never for UTF-8.
- **rar** — `PathBuf::from(bytes)`; no decode at all.
- **7z** — new `PathBuf::from_utf16` (UTF-16 → WTF-8, lossless), with the
  `\` → `/` normalisation moved *before* the conversion.
- `name_for_string_writer` is deleted; `archive::create`'s `# Errors` now
  documents that `InvalidArgument` is an `ar`-representability failure only.

**Regression test:** `ar::self_test`'s `test_byte_names` round-trips a short
(inline) and a long (GNU-table) member name containing `0x80`/`0xFE`, checks a
name ending in a space survives, and asserts `mkar` refuses each of the three
unrepresentable shapes.

### EEVDF-PICK-ON. EEVDF backend `pick_next` O(n) worst-case — RESOLVED 2026-07-15 (option (b) split-index rewrite)

**Status:** RESOLVED. `pick_next` is now amortised **O(log n)**, satisfying
CLAUDE.md's hard rule ("`pick_next` must be O(1) or O(log n) — never O(n)").
The secondary `min_vruntime`-approximation defect is fixed too. Kept below for
history and to document the design.

**Original problem (2026-07-01):** The run queue is a
`BTreeMap<(virtual_deadline, TaskId), EevdfEntry>` ordered by *deadline*, but a
task is *eligible* only when `vruntime <= min_vruntime`. The old `pick_next`
walked the tree from the front (earliest deadline) until it found the first
eligible task. Because the earliest-*deadline* tasks can be ineligible (higher
vruntime — e.g. a just-preempted task re-enqueued with accumulated vruntime but
an early deadline), that scan could walk past many entries: **O(n) worst-case**.
Secondary defect: `update_min_vruntime` derived its candidate from the
*earliest-deadline* task's vruntime, NOT the true minimum vruntime across the
queue, so the eligibility boundary itself was approximate.

**Fix implemented (option (b), split-index in safe std collections):** The
`tree` (deadline-keyed, all tasks) remains the source of truth, augmented by
two partition indexes plus a reverse index:
- `eligible: BTreeMap<(deadline, TaskId), ()>` — tasks with
  `vruntime <= min_vruntime`, ordered by deadline. `pick_next`'s Phase-1
  "earliest-deadline eligible task" is `eligible.iter().next()` = **O(log n)**.
- `ineligible_by_vrt: BTreeMap<(vruntime, TaskId), ()>` — the rest, ordered by
  vruntime. Its front is the smallest vruntime among ineligible tasks, which
  (a) feeds the true-minimum `min_vruntime` computation and (b) is the next
  candidate to promote as the floor rises.
- `deadlines: BTreeMap<TaskId, deadline>` reverse index so a task can be found
  in `tree`/`eligible` by id when promoting from `ineligible_by_vrt`.
- each `EevdfEntry` carries `is_eligible: bool` so removals (`dequeue`,
  `steal`, stale re-enqueue) hit the correct partition map in O(log n).

`update_min_vruntime` now sets the floor to the true minimum vruntime across
the ineligible set and the running task (only when `eligible` is empty, since a
non-empty eligible set means the floor is already at/above those vruntimes),
and stays monotonic. `rebalance()` drains `ineligible_by_vrt` from its front
into `eligible` while `front.vruntime <= min_vruntime`; because a waiting task's
vruntime is fixed and `min_vruntime` is monotonic, each task promotes **at most
once per residency**, so `rebalance` is amortised O(log n) per operation. It is
called after every mutation that can move the floor (`enqueue`, `dequeue`,
`tick`, `steal`, `pick_next`). Phase-2 fallback ("no eligible task → earliest
deadline overall") is `tree.iter().next()` = O(log n).

**Tests added (`eevdf::self_test`, all passing in boot self-test):**
"partition invariant holds across operations" (checks
`eligible.len()+ineligible_by_vrt.len()==tree.len()==nr_running` and
`is_eligible == (vruntime<=min_vruntime)` for every entry after each op),
"pick_next is deadline-correct under adversarial vruntime mix" (the exact case
that used to force the O(n) scan), and "min_vruntime tracks the true minimum,
not earliest-deadline". The pre-existing "weighted fairness" test was corrected
to assert on **CPU time (ticks consumed)** rather than pick *count*: with the
now-correct `min_vruntime`, a high-weight and low-weight task alternate picks
1:1, but the high-weight task runs a full slice while the low-weight one is
preempted early — weighted fairness correctly manifests as more CPU time, not
more picks. (The old pick-count assertion only passed by accident of the old
`min_vruntime` bug.)

### TD32. Container rootfs jail uses the extracted `lower` dir directly (no overlay CoW) and only jails absolute paths

**Where:** `kernel/src/kshell.rs` (`oci run`, `cmd_oci`) sets the container's
`root_path` to the extracted `/tmp/oci-<name>/lower` tree;
`kernel/src/ipc/namespace.rs` (`apply_root`). The `fs::overlay` module exists and
`oci run` *creates* an overlay (lower+upper) but the overlay is ID-addressed, not
mounted into the VFS path tree, so the per-process root jail (which prepends a
host path prefix and routes through the normal VFS) cannot resolve through it.

**The debt.**
1. **No copy-on-write isolation.** Because the jail points at `lower`, writes the
   container makes land in the shared extracted image tree, not the per-container
   `upper`. Two containers from the same image would see each other's writes, and
   `overlay reset`/`commit` semantics don't apply to the running container.
2. **Relative paths are not jailed.** `apply_root` only re-anchors absolute
   paths; relative paths pass through for a per-process cwd layer to resolve. That
   cwd layer does not yet jail cwd, so a container process using relative paths
   from an unjailed cwd could currently resolve outside its root. The image
   entrypoint and its libraries use absolute paths, so this doesn't bite the
   common launch path, but it is a real containment gap.

**Why it didn't block increments 3–4 (§42):** the entrypoint binary and its
libraries are read via absolute paths under the rootfs, which `apply_root` jails
correctly (`..` clamped), so launching a statically-linked image entrypoint
works and is isolated for reads. The gaps are CoW write-isolation and
relative-path containment.

**Proper fix.** (a) VFS-mount the overlay at the container's rootfs mountpoint so
the jail routes through copy-on-write (writes → `upper`, reads → merged), i.e.
give `fs::overlay` a real VFS mount adapter and point `root_path` at the merged
mountpoint instead of `lower`. (b) Jail cwd end-to-end: make the per-process cwd
itself a jailed (absolute, within-root) path so relative resolution is contained,
then have `apply_root` (or the cwd-join layer) treat relative paths as
rooted-after-join. Track alongside the mount-namespace/`pivot_root` work deferred
in §42.

**Update 2026-06-30 (increment 5):** Part (a)'s blocker is removed. The
`fs::overlay::OverlayFs` VFS mount adapter now exists and works — but only after
fixing a foundational VFS issue: the global VFS lock was held across every
filesystem method call, so mounting an overlay (whose methods re-enter the VFS to
read their backing layers) deadlocked on boot. The VFS now uses a **per-mount
lock** (`Arc<Mutex<Box<dyn FileSystem>>>` + `resolve_mount`; design-decisions
§43), so stacked filesystems mount cleanly (overlay self-test 13 passes). **Still
open for TD32:** wiring `oci run`/`container create` to actually mount an
`OverlayFs` at the container rootfs and point `root_path` at that mountpoint
instead of `lower` (increment 6), plus part (b) cwd jailing.

**Update 2026-06-30 (increment 6): part (a) DONE.** `oci run` now VFS-mounts the
per-container `OverlayFs` adapter at `/containers/<name>/rootfs` and jails the
container at that merged mountpoint (not the read-only `lower`), so container
writes are copy-on-write isolated — reads see the merged view, writes land in the
per-container `upper` layer. The overlay creation (`fs::overlay::create`) now
flows its `OverlayId` into the mount step; if the overlay can't be created or
mounted, the launch gracefully falls back to jailing at the read-only `lower`.
The mountpoint is recorded on the `Container` (`rootfs_mount` field +
`set_rootfs_mount` setter, Created-only) and `container::delete` unmounts it on
teardown (outside the table lock; the VFS has its own per-mount locking). Both the
entrypoint-ELF read and the jail now route through `jail_root`.
**Still open for TD32:** part (b) — cwd jailing (relative-path containment). The
absolute-path read isolation and now CoW write isolation are both in place; the
remaining gap is jailing a container process's *cwd* so relative resolution is
contained, alongside the mount-namespace/`pivot_root` work deferred in §42.

**Update 2026-06-30 (increment 7): double-jail bug in fd-backed I/O — FIXED.**
While preparing part (b) we discovered that *all* fd-backed file I/O was broken
for jailed (container) processes — a regression that increment 6's CoW mount
would have exposed the moment a container actually opened a file. Root cause:
`namespace::apply_root` is intentionally **non-idempotent** (it blindly prefixes
the jail root, assuming a *guest* path), but `handle::open()` stored the
*already-resolved host path* in the file handle (`file.path`), and every
subsequent handle op (`Vfs::read_at(&file.path)`, `write_at`, `truncate`,
`metadata`, `readdir_at`, `file_identity`, `flock`/`funlock`, …) called
`resolve_follow` *again* → re-applied the jail prefix → double-jailed to a path
that doesn't exist. For a jailed process even `open()` failed (its internal
`stat`/`truncate`/`write_file` re-jailed). Non-jailed processes were unaffected
only because `resolve_follow` is idempotent on already-resolved non-jailed paths.
**Fix (design-decisions §44):** every path-based `Vfs` method is split into a
thin wrapper (`resolve_follow` → call worker) plus a `*_resolved` worker that
operates on an already-resolved host path *without* re-translating. Handle-backed
ops call the `*_resolved` worker directly (an open fd holds a resolved reference —
Unix semantics, immune to later chroot/rename/symlink changes). Split methods:
`read_at`, `read_file`, `stat`, `write_file`, `write_at`, `truncate`, `metadata`,
`read_at_uncached`, `readdir_at`, `file_identity`, `flock`, `funlock`,
`lock_query`. A non-idempotency guard was added to
`namespace::test_process_root` (re-resolving an already-jailed path must
double-jail) to pin the invariant so a future refactor that makes handle ops
re-resolve is caught at boot. Build clean, clippy delta zero, boot-test green.

**Update 2026-06-30 (increment 8): part (b) cwd / relative-path containment —
DONE.** TD32 part (b) is closed. Relative paths are canonicalized against the
per-process cwd in the syscall layer *before* the VFS jails them, so containment
hinges entirely on cwd (and dirfd base paths) being stored as **guest** paths.
`chdir` already stored a guest cwd, but three sites stored/used the *resolved
host* path and so leaked the jail location (`getcwd`) and double-jailed relative
resolution: (1) `fchdir` stored `handle_path` (host) as cwd; (2) `sys_openat`
with a real dirfd built `host_dir + rel` then re-jailed it (and its directory
type-check `stat(&host)` re-jailed → ENOENT for every relative `*at` from a
jailed process); (3) `resolve_at_path` (the shared `*at` resolver:
fstatat/unlinkat/fchownat/…) had the identical defect. **Fix
(design-decisions §45):** added `namespace::unjail_path_for(pid, host) → guest`
(exact inverse of `apply_root`: strips the jail-root prefix; no-op when
unjailed). `fchdir` now stores the un-jailed guest cwd. A new shared helper
`dirfd_to_guest_dir(dirfd)` resolves a real dirfd to its *guest* directory path,
doing the directory-type check with `stat_resolved` (no re-jail); both
`sys_openat` and `resolve_at_path` use it, so the combined path is jailed
exactly once. Round-trip regression assertions
(`unjail(resolve(guest)) == normalized guest`, unjailed no-op, out-of-jail
defensive passthrough) added to `namespace::test_process_root`. **Limitation:**
`unjail_path_for` reverses only the chroot layer, not namespace Bind/Hide
remapping — the container runtime never combines Bind rules with a chroot jail,
so the reversal is exact for the container case (documented on the function and
in §45). With parts (a) [CoW, inc 6] and (b) [this] done, TD32's remaining scope
is the broader mount-namespace/`pivot_root` work deferred in §42 (a separate,
larger feature, not a containment gap).

**Update 2026-06-30 (increment 9): volume (bind) mounts — DONE.** The first
concrete slice of TD32's remaining mount-namespace scope landed. A per-process
volume table (`PROCESS_MOUNTS` in `namespace.rs`) layers Docker `-v`-style bind
mounts *over* the chroot: a guest path under a volume prefix resolves to an
arbitrary host target (escaping the rootfs), while everything else still jails
under the rootfs. Volume matching runs *after* `..`-normalization, so a guest
cannot climb out of a volume into the host (security-critical ordering).
`unjail_path_for` reverses volumes too (longest host-target match), so `fchdir`
into a volume reports the guest path and stays single-jailed. Container plumbing:
`Container.volumes` + `add_volume_mount()` (Created-only, `-v` order), installed
on the init process in `add_process_task`, cleared in `remove_process_task`/
`delete`/`detach`. Covered by `namespace::test_volume_mounts` and container
self-test 19; build/clippy clean, boot-test green. Design rationale in §46.
Still deferred (TD32 remainder): a true longest-prefix mount-tree that subsumes
the rootfs as the `/` mount (the `pivot_root` target), read-only volumes
(`-v …:ro`), and tmpfs/named-volume types — all straightforward extensions on
the same table.

**Update 2026-06-30 (increment 10): `-v` CLI flag — DONE.** The volume
mechanism now reaches end-to-end from the shell: `oci run <dir> -v
/srv/data:/data` (also `--volume`, repeatable) parses each spec on the first
`:` (Docker order), validates both sides are absolute, and installs the bind
mount via `add_volume_mount` while the container is still in Created state —
before the init process launches. Usage/help strings updated. Container
self-tests 18/19 were also made deterministic this session (synthetic
never-scheduled PID instead of a real init process that could exit mid-test and
clear its namespace — see B-CONTAINER-JAIL-TESTRACE). Build clean, boot-test
green ("Self-test PASSED (19 tests)"). The TD32 remainder above (read-only
volumes, mount-tree/`pivot_root`, tmpfs) is unchanged.

**Update 2026-06-30 (increment 11): port publishing (`-p`) — DONE.** Docker
`-p host:container[/proto]` port publishing landed, reusing the existing
`net::nat` port-forward table. `Container` gained `container_ip` (captured from
the configured network IP) and `published_ports`; `add_port_publish` records
publish intents (Created-only, requires a network IP, rejects port 0, last-
writer-wins, capped at `MAX_PUBLISHED_PORTS`); `run()` installs them as NAT
rules forwarding host traffic to the container IP inside its netns; `stop()`
flushes them and `delete()` clears the intents. CLI: `oci run -p
8080:80[/udp]` (repeatable). Container self-test 20 covers the lifecycle
deterministically (forwards are per-netns, not per-PID). This is orthogonal to
the rootfs/volume mount-namespace scope; the TD32 mount remainder (read-only
volumes, mount-tree/`pivot_root`, tmpfs) is still open.

**Update 2026-06-30 (increment 12): env injection (`-e`) — DONE.** Docker
`-e KEY=value`/`--env` environment injection landed entirely in the CLI launch
path (`kshell::cmd_oci` `run`/`create`); the container/kernel layer needed no
change because env already passes through `SpawnOptions::envp`. The parser
requires `KEY=value` (a bare `-e KEY` is rejected — a container has no host
environment to inherit) and rejects an empty key. At launch the CLI `-e` entries
are merged over the image's declared ENV with Docker override semantics: each
`-e` entry wins over an image ENV entry with the same key, and the merged set has
no duplicate keys (CLI entries added first, then image ENV entries whose key is
not already overridden). Usage/help strings updated to include `[-e KEY=value
...]`. The TD32 mount remainder (read-only volumes, mount-tree/`pivot_root`,
tmpfs) is still open.

**Update 2026-06-30 (increment 13): `docker`/`dk` CLI-compat shim — DONE.** A
thin Docker-CLI front-end (`docker`, alias `dk`) translates familiar verbs to
the native `oci` (image) and `container` (lifecycle) handlers: `run`/`create`
→ `oci run`/`create`; `ps [-a]` → `container list` (all states; `-a` accepted +
ignored since there is no running-only index); `start`/`stop`/`rm` →
`container start`/`stop`/`delete`; `inspect` → `container info`; `exec` →
`container exec`; `images <dir>` → `oci inspect` (SlateOS has no name-keyed
image registry — images are on-disk OCI layout dirs). Argument spacing is
preserved verbatim when delegating. Registered in dispatch, `is_builtin`, and
the tab-completion list.

**Update 2026-06-30 (increment 14): resource limits (`--memory`/`--cpus`) —
DONE.** `oci run`/`create` now accept Docker `--memory`/`-m <SIZE>` (bytes with
optional binary k/m/g[b] suffix, rounded up to whole 16 KiB frames → cgroup
`mem_limit`) and `--cpus <N[.M]>` (fractional cores → percent of one core, e.g.
`1.5` → 150 → `CpuLimit::from_percent` via cgroup `cpu_quota`). Parsing is pure
and float-free (kernel has no FPU state in this path); two helpers
(`parse_mem_size_to_frames`, `parse_cpus_to_percent`) are covered by
`kshell::cli_resource_parser_self_test()`, wired into the boot self-test run in
`main.rs`. The TD32 mount remainder (read-only volumes, mount-tree/`pivot_root`,
tmpfs) is still open.

**Update 2026-06-30 (increment 15): read-only volumes (`-v …:ro`) — DONE.**
Docker `-v host:guest[:ro|:rw]` now carries an access mode end-to-end. The
volume table entry (`VolumeMount` in `namespace.rs`, `VolumeSpec = (guest,
host, read_only)` in `container.rs`) gained a `read_only` flag; `add_volume`
and `add_volume_mount` take it (last-writer-wins, so re-mounting the same guest
prefix `:rw` clears a prior `:ro`). Enforcement is a new
`namespace::check_writable(path)` / `check_writable_for(pid, path)` that mirrors
the exact resolution pipeline used by `resolve_path_for` — step-1 namespace
translation, `..`-clamping `normalize_jailed`, then longest-prefix volume match —
and returns `KernelError::ReadOnlyFilesystem` (EROFS) when the matched volume is
read-only. It is a cheap `Ok(())` no-op for any process without volumes or
without a chroot root (all non-container processes, and containers with only
read-write volumes), making the wide enforcement surface zero-risk to existing
behavior. Two chokepoints gate writes: (1) fd-based writes via
`fs::handle::open()` reject up front when the open flags request write/create/
truncate/append; (2) ~17 path-based mutating `Vfs` methods (`write_file`,
`write_at`, `truncate`, `remove`, `remove_recursive`, `mkdir`, `mkdir_all`,
`rmdir`, `rename`/`rename_noreplace` via `rename_inner`, `rename_exchange`,
`set_permissions`, `set_times`, `set_xattr`, `remove_xattr`, `symlink`, `link`,
`atomic_write`) call the namespace check on the caller's (guest) path before
host-path resolution. The `_resolved` variants are intentionally *not* gated
(they take already-translated host paths). CLI: `oci run -v /srv/data:/data:ro`
parses an optional third `:mode` segment (`ro`/`rw`, default `rw`); unknown
modes are rejected. Covered by `namespace::test_volume_mounts` (read-only volume
write-denied / read-allowed assertions) and container self-test 19
(`check_writable_for` on `/logs` ro vs `/data` rw vs `/bin/sh` rootfs).
The TD32 mount remainder (a true longest-prefix mount-tree subsuming the rootfs
as the `/` mount / `pivot_root` target, `--read-only` root, and tmpfs/named-
volume types) is still open.

**Update 2026-06-30 (increment 16): read-only root (`--read-only`) — DONE.**
Docker `--read-only` now makes the whole container rootfs non-writable while
writable (`:rw`) volumes still punch writable holes through it. A per-process
flag set `PROCESS_ROOT_RO` in `namespace.rs` (set via `set_root_read_only(pid,
ro)` / queried via `is_root_read_only`, cleared on `detach`/`clear_root` for
PID-reuse safety) feeds the same `check_writable_for` decision used for `:ro`
volumes: longest-prefix volume match first (a `:ro` volume → EROFS, a `:rw`
volume → allowed), and when *no* volume matches the path lives in the rootfs, so
it is denied iff the root is read-only. The fast-path `Ok(())` no-op now also
requires a writable root, so non-container processes and writable containers are
still zero-cost. `ContainerConfig` gained a `read_only_root` field + `.read_only(bool)`
builder; the flag rides through `create` → `add_process_task`, which calls
`set_root_read_only(pid, true)` after installing volumes (only when a chroot root
exists). Post-create `container::set_read_only_root(id, ro)` (Created-state-gated,
like `set_root_path`) mirrors the volume setter; `ContainerInfo` reports it. CLI:
`oci run … --read-only` (a bare flag) prints `Root FS: read-only`. Covered by
`namespace::test_volume_mounts` (read-only-root block: rootfs denied, `:rw`
volume still writable, flag-clear restores writability) and container self-test
19b (now 21 tests total). The TD32 mount remainder is now just the true
longest-prefix mount-tree subsuming the rootfs as the `/` mount (`pivot_root`
target) and tmpfs/named-volume types.

**Update 2026-07-01 (increment 17): tmpfs mounts (`--tmpfs`) — DONE.** Docker
`--tmpfs /guest` now mounts an ephemeral in-memory filesystem at a guest path.
Modeled as a bind mount whose host target is a per-container `fs::memfs` mount:
`add_tmpfs_mount(id, guest)` (Created-only) validates the guest path (absolute,
not `/`, no duplicate against existing volumes/tmpfs), then — outside the table
lock — `Vfs::mkdir_all` + `memfs::mount` a fresh in-memory fs at a unique host
mountpoint `/var/lib/slate/tmpfs/<id>-<index>`, and records it as a **writable**
`VolumeSpec` at the guest prefix so all the existing volume resolution/write
machinery (`resolve_path_for`, `check_writable_for`, `..`-clamping) applies
unchanged. The `Container` gained a `tmpfs_mounts: Vec<String>` of owned
mountpoints; `delete()` unmounts and `remove_recursive`-removes each so nothing
leaks. CLI: `oci run … --tmpfs /tmp` (repeatable) — mount **options** (`--tmpfs
/tmp:size=64m`) are explicitly rejected with a warning rather than silently
ignored (an unbounded tmpfs is a containment/DoS gap until per-mount quota
enforcement lands; honest failure until then). Covered by container self-test 46
(two mounts, bad-spec/duplicate rejection, writable-memfs write+read-back,
non-Created rejection, delete-unmount verification — now 60 tests total). Build/
clippy clean, boot-test green. With this, the volume *types* are all covered —
host bind mounts (`-v /host:/guest`), read-only volumes (`:ro`), named volumes
(`-v NAME:/guest` via `volume::ensure`), and now tmpfs (`--tmpfs`). The TD32
mount remainder is therefore now just the true longest-prefix mount-tree
subsuming the rootfs as the `/` mount (the `pivot_root` target) — the last
structural piece, not a volume-type gap.

**Update 2026-07-01 (increment 18): container-aware `/proc/<pid>/mountinfo` —
DONE.** A container (jailed) process now sees *its own* mount view in
`/proc/<pid>/mountinfo` instead of the host's global mount table. Previously
`gen_pid_mountinfo` rendered `Vfs::mounts_full()` for every PID, so a process
inside a container observed the entire host mount topology (an info leak) and
none of its own rootfs/volumes/tmpfs (a correctness gap). Fix:
`namespace::mount_view_for(pid)` returns `None` for an unjailed process (keep
the global table) or the container's ordered view — the rootfs at guest `/`
(read-only iff `--read-only`), then each volume/tmpfs at its guest prefix with
its own `:ro`/`:rw` flag. `procfs::render_container_mountinfo` renders it,
resolving each entry's *fstype* from the real host mount backing its
`host_target` (`fstype_for_host_path` longest-prefix match: overlay for the
rootfs, tmpfs/memfs for `--tmpfs`, the host fs for binds) while reporting the
`source` field as `none` so host backing paths are **not** leaked into the
container. The same container-aware rendering was applied to the `/proc/mounts`
line format (`render_container_mounts`): the global `/proc/mounts` now resolves
the *caller's* view (`current_task_id`), and a new per-PID `/proc/<pid>/mounts`
(hence `/proc/self/mounts`) file mirrors Linux's mount-namespace-local table.
Covered by procfs self-tests (container view for both `mountinfo` and `mounts`:
RO rootfs→overlay, RO bind→ext4, RW tmpfs→tmpfs; plus `mount_path_covers`
boundary safety so `/data` doesn't cover `/database`). Build/clippy clean,
boot-test green. Note this is *introspection* only — real in-container
`mount`/`umount`/`pivot_root` syscalls mutating a per-container mount table
remain the deferred mount-namespace piece.

### TD33. Container `logs` capture works only for Linux-ABI container inits — ACCEPTED LIMITATION 2026-06-30

**Where:** `kernel/src/container.rs` (`open_capture_log`, whose handle
`run_with_abi` passes as fd 1 + fd 2 to `spawn_process_with_redirects` /
`spawn_process_with_abi_and_redirects`). The capture works by installing the
capture handle into the init process's **Linux fd table** as fds 1 and 2 (one
shared handle — dup2 semantics — for interleaved stdout+stderr). *(Updated
2026-07-22: the redirect is now installed atomically inside the spawn, before
the child is runnable, closing the former post-spawn TOCTOU window; the old
`redirect_output_to_capture` post-spawn helper was removed. See the
B-PTHREAD-YIELDBUDGET empty-capture entry for the race fix.)*

**The limitation.** The `linux_fd_table` is only installed for **Linux-ABI**
binaries (`spawn.rs`: `if is_linux_abi { … linux_fd_install_stdio(pid) }`).
Native SlateOS binaries have no `linux_fd_table`, so the spawn redirect applier
skips installation (and closes the handle instead), the container's `log_path`
stays empty, and `container logs ID` returns `NotFound`. A native-ABI container
init's stdout/stderr therefore goes to the console and is **not** captured to
`/var/log/containers/<id>.log`.

**Why it's accepted, not blocking.** Real Docker/OCI container entrypoints are
Linux-ABI glibc ELFs, which is exactly the path the capture supports. The
native-ABI container init is a SlateOS-specific corner case (no real image ships
one), so the Docker-compatible `logs` feature is correct and sufficient for its
intended use. The self-test (19t) deliberately forces `AbiMode::Linux` via
`run_with_abi` so it exercises the real capture path deterministically.

**Proper fix (deferred).** Also wire capture through the **native** fd-inheritance
channel (`initial_fds` / `SpawnOptions.fd_map`, consumed via
`SYS_PROCESS_GET_INITIAL_FDS`): install the capture handle as fd 1/2 in the
native init's `initial_fds` when the ABI is Native. Deferred because it needs
verification that native binaries honour `initial_fds` for stdout and that the
file-offset-sharing (single append position for interleaved 1+2) semantics match
the Linux-fd path — unverified today, and shipping it unverified would violate
the no-band-aid rule. Trigger to do it: a real native-ABI container init appears,
or `initial_fds` stdout semantics are confirmed.

### TD31. Cgroup `nr_tasks` accounting is attach/detach-symmetric only, not membership-accurate — RESOLVED 2026-07-02

**RESOLUTION (2026-07-02).** Made membership counting symmetric with task
lifetime. The **detach half** had already landed in `reap_dead_tasks` (commit
`d7b926037`, 2026-07-01): a reaped task in a non-root cgroup calls
`cgroup::detach_task(task_cgroup)` after `drop(state)` (SCHED released → TABLE,
preserving lock order). This 2026-07-02 change adds the matching **attach half**
in `sched::spawn_with_affinity`: after the `without_interrupts`/SCHED critical
section ends and SCHED is dropped, a task that inherited a non-root cgroup calls
`cgroup::attach_task(inherit_cgroup)` (ROOT skipped, matching the reap-side skip;
TABLE taken strictly after SCHED). Because *all* task creation (kernel and user)
funnels through `spawn_with_affinity` (`proc::thread::spawn_user` →
`thread::spawn` → `sched::spawn` → `spawn_with_affinity`), this single site makes
every fork/clone/spawn counted and every reap decremented — a true membership
count. Tasks bound via `set_task_cgroup` (e.g. a container init, which inherits
ROOT at spawn so the spawn-attach is skipped, then is explicitly bound) stay
balanced: attach at bind, detach at reap.

**Why it's now safe (was BLOCKED on a boot hang).** The earlier attempt hung the
boot twice because the extra `TABLE` lock traffic aggravated
**B-PREEMPT-SPINLOCK** — a `crate::sync::Mutex` held across an involuntary
preemption could deadlock against a higher-priority spinner on a single CPU. That
root cause was fixed 2026-07-01 (per-CPU `PREEMPT_DISABLE_COUNT`: a tracked mutex
now disables preemption while held). With that fix, re-applying the attach edit
booted **green 4× consecutively** (baseline 190s + 182s/181s/185s), zero hangs,
zero `SPINLOCK STALL`, zero self-test failures, and no `dash`/`pthread` flakes —
exactly the retry trigger this entry documented. `cgroup::delete`'s
`nr_tasks > 0 ⇒ NotEmpty` guard is now a true "container still has live
processes" check.

---

**Original entry (for context):**

**Where:** `kernel/src/cgroup.rs` (`attach_task`/`detach_task`/`stats.nr_tasks`),
`kernel/src/sched/mod.rs` (`sched::spawn` ~L1046 sets `new_task.cgroup_id` on
creation but does **not** call `cgroup::attach_task`; `reap_dead_tasks` ~L2789
removes a dead task without `cgroup::detach_task`). The single authoritative
mover `set_task_cgroup` *does* keep the counts balanced (detach old, attach new).

**The debt.** `nr_tasks` only counts tasks that were *explicitly moved* via
`set_task_cgroup`. Two asymmetries:
1. **Creation:** a task that simply *inherits* its creator's `cgroup_id`
   (the common case — every fork/clone/spawn) bumps no counter, so a busy
   cgroup can report `nr_tasks == 0` while hosting many tasks.
2. **Death:** when a task is reaped, its cgroup's `nr_tasks` is never
   decremented (and `set_task_cgroup`-style moves to ROOT on container
   `remove_process` leave ROOT's count permanently inflated, since the task is
   then killed without a matching detach).

`detach_task` saturates at 0 so neither asymmetry can panic/underflow, but the
counter is unreliable for anything that needs a true membership count (e.g. a
cgroup "no new forks past a task limit" controller, or `cgroup.procs`-style
introspection).

**Why it didn't block container increment 1 (§41):** `container::run` binds the
init task via `set_task_cgroup`, which *does* increment the container cgroup, so
the end-to-end "process billed to container cgroup" assertion (`nr_tasks == 1`)
holds. The self-test cleanup calls `remove_process_task` (a `set_task_cgroup` to
ROOT) *before* killing the task, so the container cgroup returns to 0 and
`delete()` (which requires `nr_tasks == 0`) succeeds.

**Proper fix.** Make membership counting symmetric with task lifetime, not with
explicit moves: call `cgroup::attach_task(inherit_cgroup)` in `sched::spawn` when
a new task adopts a cgroup, and `cgroup::detach_task(task.cgroup_id)` in
`reap_dead_tasks` (after dropping the SCHED lock, honoring the SCHED → cgroup
lock order). Audit ROOT_CGROUP bootstrapping so the idle/boot tasks are counted
consistently. Once symmetric, `cgroup::delete`'s `nr_tasks > 0 ⇒ NotEmpty` guard
becomes a true "container still has live processes" check.

**ATTEMPTED 2026-07-01 — BLOCKED on a boot hang the change triggers/exposes.**
Implemented exactly the proper fix above: `attach_task(inherit_cgroup)` in
`spawn_with_affinity` (after the `without_interrupts`/SCHED critical section, so
the cgroup `TABLE` lock is taken strictly after SCHED, mirroring
`set_task_cgroup`'s order) and `detach_task(task.cgroup_id)` in `reap_dead_tasks`
(capture `task.cgroup_id` under SCHED, `drop(state)`, then detach — TABLE after
SCHED). It builds clean, clippy-0, and the *normal* container lifecycle self-test
(nr_tasks 0→1→0) still passes. **But two consecutive boot tests hung** (BOOT_OK
never printed within 480 s), each time immediately after a **userspace container
init process** was spawned and marked "running" — run #1 hung in the
`container restart` self-test (after `test-restart-ct` task 185), run #2 in the
`container port` self-test (after `test-port-ct` task 187). Reverting *only* the
two sched edits → BOOT_OK reached in 181 s. So the change is the trigger; the
varying hang location within a boot points to a **near-deterministic SMP timing
race** in the process spawn/force-kill/reap path that the *extra cgroup-`TABLE`
lock traffic* (one attach per spawn, one detach per reap) aggravates rather than
a plain AB-BA deadlock (SCHED and `TABLE` are never held nested; charging holds
frame-lock→`TABLE` while reap does `TABLE`→frame-lock but with `TABLE` released
in between, so no static inversion was found by inspection). Note the boot is
*already* mildly flaky independent of this change: the reverted-sched boot run
saw an unrelated `dash script-from-stdin` self-test `InternalError` (see the
dash-flake entry) — consistent with a pre-existing timing fragility in the
ring-3 spawn/reap machinery that this change amplifies.

**Decision (Claude, autonomous):** do NOT land the symmetric-accounting change
until the underlying spawn/kill/reap race is root-caused, because it regresses
boot stability, and the debt it fixes is cosmetic (stale `nr_tasks` for
force-killed-unreaped tasks; `container::delete` ignores the `cgroup::delete`
NotEmpty error with `let _ =`, so accounting drift never blocks teardown). The
`nr_tasks==1` container-billing assertion and the D-CGROUP-TASK-UNASSIGNED
end-to-end memory-charging test both pass without it. **Trigger to retry:** after
the ring-3 spawn/reap SMP race is instrumented (per-lock acquire/spin counters or
a lock-order tracer) and fixed; then re-apply the two sched edits and run the
boot test ≥3× to confirm stability. The exact patch is small and is captured
above so it can be reconstructed.

### TD30. Console TTY line discipline: `^C`/`^\`/`^Z` signal the fg pgrp (canonical + raw), `VMIN`/`VTIME` + `NOFLSH` honoured, orphan-pgrp `SIGHUP`/`SIGCONT` — RESOLVED 2026-06-20

**Where:** `kernel/src/tty.rs` — `feed()` (canonical line editor) and
`raw_read()` (non-canonical reader); driven by `dispatch_console_read` /
`deliver_console_signal` / `console_terminal_ioctl` in
`kernel/src/syscall/linux.rs`.

**RESOLVED — gap (1) `ISIG` signal generation (`^C`/`^\`):** the console
now has a foreground process group and delivers terminal signals to it.
`tty.rs` gained a `FOREGROUND_PGID` atomic with
`foreground_pgid()`/`set_foreground_pgid()`, the `TIOCGPGRP` (0x540F) /
`TIOCSPGRP` (0x5410) ioctls (`tcgetpgrp`/`tcsetpgrp`), and a
`ConsoleRead{Data(n)|Signal(sig)}` return from `console_read`. On a
`^C`/`^\` in canonical mode (`feed` → `LineStep::Signal`),
`deliver_console_signal()` resolves the foreground pgrp via
`pcb::pids_in_group` and posts `SIGINT`/`SIGQUIT` (with `SI_KERNEL`
siginfo) to every member, then returns `ERESTARTSYS` so the blocked
reader's signal checkpoint runs — a transparent restart when the reader
isn't in the fg group (or the handler has `SA_RESTART`), otherwise the
default action / `-EINTR`. With no foreground group installed
(`pgid == 0`) no signal is generated and the read simply restarts.

**RESOLVED — Ctrl-Z (`VSUSP`) → `SIGTSTP`:** `feed()` now recognises
`VSUSP` under `ISIG` (default `^Z`) and returns `LineStep::Signal(20)`,
flushing the in-progress line like `^C`/`^\`. `deliver_console_signal`
routes `SIGTSTP` to the foreground pgrp, whose `DefaultAction::Stop`
(already implemented in `proc::signal`) suspends the job; a later
`SIGCONT` (shell `fg`/`bg`) resumes it. `NOFLSH` is not yet honoured.

**RESOLVED — `VTIME`:** `raw_read()` now honours all four `(VMIN, VTIME)`
combinations per POSIX. A new `keyboard::read_char_timeout(deadline_ns)`
(HLT-yield loop bounded by an `hrtimer::now_ns()` deadline) backs the two
timed cases: `VMIN=0,VTIME>0` (bounded read timeout on the first byte) and
`VMIN>0,VTIME>0` (inter-byte timer restarted after each byte, first byte
blocking). `VMIN=0,VTIME=0` (poll) and `VMIN>0,VTIME=0` (count) are
unchanged. VTIME is interpreted in deciseconds.

**RESOLVED — raw-mode `ISIG`:** `raw_read()` now classifies each byte
against `VINTR`/`VQUIT`/`VSUSP` when `ISIG` is set (in all four
`(VMIN,VTIME)` arms) and returns `ConsoleRead::Signal`, discarding any
bytes collected so far in the call (input flush — see the `NOFLSH` note
below for why this is unconditional in raw mode).  Apps that clear `ISIG`
(most full-screen programs) still get the characters as literal data.

**RESOLVED — orphaned-process-group `SIGHUP`/`SIGCONT`:** POSIX requires
that when a process exit orphans a process group that still contains a
*stopped* member, that group be sent `SIGHUP` then `SIGCONT` so wedged
jobs are not stuck forever with no shell able to continue them. Now
implemented in the process-exit path rather than tied to a
controlling-terminal model: `pcb::guarded_child_pgrps(pid)` captures the
distinct groups `pid` *guards* (children in a different group but the same
session) **before** `remove_thread` reparents them to init;
`thread::on_thread_exit` re-checks each captured group after the process
zombifies via `pcb::pgrp_orphaned_with_stopped(pgid)` — true only when no
live member has a guardian (a live parent in a different group of the same
session; zombies count as neither member nor guardian) *and* some member
is stopped — and calls `handlers::kill_orphaned_pgrp(pgid)`, which sends
`SIGHUP` then `SIGCONT` to every member via the authority-free
`handlers::deliver_kernel_signal` (classify → default action). Covered by
the `pcb::test_orphaned_pgrp` boot self-test (guarded-vs-orphaned and the
no-stopped-member negative case).

**RESOLVED — `NOFLSH`:** `feed()` now honours the `NOFLSH` (0x80) lflag in
canonical mode: a signal character (`^C`/`^\`/`^Z`) flushes the in-progress
line by default, but with `NOFLSH` set the buffered input is preserved and
only the signal is generated (the line then completes normally on the next
newline). Raw mode keeps no kernel-side input queue across `read(2)` calls
(each call reads straight from the keyboard), so there is no buffered input
for `NOFLSH` to preserve there — documented on `raw_read`. Covered by the
`tty` boot self-test (NOFLSH-preserves-line) and a `#[cfg(test)]` unit test.

**Severity:** none remaining — interactive `^C`/`^\`/`^Z` (canonical and
raw), `VMIN`/`VTIME` raw reads, orphaned-process-group hangup, and `NOFLSH`
all work (once a shell installs a foreground pgrp via `tcsetpgrp`).

### TD29. Linux signal `siginfo` sender-class (`si_code`/`si_pid`/`si_uid`) — RESOLVED 2026-06-15

**Resolution:** Implemented sender-faithful `siginfo`. `SignalState`
(`kernel/src/proc/signal.rs`) now carries a per-signal `Option<SigInfo>` array co-located
under the same lock as the pending bitmap, recorded on the clear→set transition
(coalescing first-wins, matching Linux's standard-signal `struct sigqueue` behaviour) and
taken at delivery. `SigInfo { code, sender_pid, sender_uid, value }` is threaded through the
post funnel: `kill(2)` → `SI_USER` + sender pid/uid; `tkill`/`tgkill` (`raise`/`pthread_kill`)
→ `SI_TKILL` + sender pid; timer expiry (`setitimer`/`alarm` SIGALRM, `kernel/src/proc/itimer.rs`)
→ `SI_KERNEL`. `build_linux_rt_frame` dequeues the matching record to fill the
`LinuxSiginfo` handed to an `SA_SIGINFO` handler. Verified by the `siginfo
record/deliver/coalesce` unit self-test (13 tests pass) and the `/bin/signal` ring-3 glibc
test, which now asserts `si_code == SI_TKILL (-6)` and `si_pid == getpid()` for `raise()`
(`SLATE_GLIBC_SIGNAL_OK signo=10 code=-6 self=1`).

**Synchronous fault `si_code`/`si_addr` — RESOLVED 2026-06-16 (follow-on to TD29).**
CPU faults on an `AbiMode::Linux` process with an installed handler are now delivered as
real Linux signals with a faithful, fault-specific `siginfo`. A shared emitter
`emit_linux_rt_frame(pid, sig, act, regs: &LinuxTrapRegs, siginfo) -> Option<RtFrameEntry>`
(`kernel/src/syscall/linux.rs`) builds the `rt_sigframe` from a neutral register snapshot, so
it is reused by both the async syscall-return path (`build_linux_rt_frame`, snapshot from the
`SyscallFrame`) and the synchronous fault path (`try_deliver_linux_fault_signal`,
`kernel/src/idt.rs`, snapshot read out of the `InterruptStackFrame` + `SavedRegisters` via
`read_volatile`). `linux_fault_mapping` classifies the trap vector → `(signo, si_code)`:
`#DE`→`SIGFPE`/`FPE_INTDIV`, `#OF`→`SIGFPE`/`FPE_INTOVF`, `#UD`→`SIGILL`/`ILL_ILLOPN`,
`#MF`/`#XM`→`SIGFPE`/`FPE_FLTINV`, `#AC`→`SIGBUS`/`BUS_ADRALN`,
`#BR`/`#NP`/`#SS`/`#GP`→`SIGSEGV`/`SI_KERNEL`; `#PF` is handled in `handle_page_fault`, which
sets `si_addr = CR2` and `si_code = SEGV_ACCERR` (protection, present bit set) or
`SEGV_MAPERR` (not mapped). For non-`#PF` faults `si_addr =` faulting RIP. The emitter does
**not** re-arm on a frame-build failure — the fault caller terminates instead, since resuming
would immediately re-fault. Native processes keep the SEH-style `SignalContext` trampoline
(design-decision #4). Verified by the `/bin/fault` ring-3 glibc self-test
(`self_test_linux_real_glibc_fault`, `kernel/src/proc/spawn.rs`): a real `#PF` store to an
unmapped `0xDEAD000` enters an unmodified glibc `SA_SIGINFO` `SIGSEGV` handler that reads
`si_signo==11`/`si_code==SEGV_MAPERR(1)`/`si_addr==0xdead000` and `siglongjmp`s out, printing
`SLATE_GLIBC_FAULT_OK signo=11 code=1 addr=0xdead000` (boot test PASSED).

**`SI_QUEUE` `si_value`/`si_ptr` payload — RESOLVED 2026-06-16 (follow-on to TD29).**
`rt_sigqueueinfo(2)`, `rt_tgsigqueueinfo(2)` and `pidfd_send_signal(2)` now read the
user-supplied `siginfo`, copy out `si_code` and the 8-byte `si_value` union
(`read_user_siginfo_payload`, SMAP-safe via `copy_from_user`), record the value on the
pending signal, and stamp it into the delivered `siginfo_t` at the correct ABI offset
(struct +24) via the new `LinuxSiginfo::queue(...)` builder; `build_linux_rt_frame`
branches to it when `si_code == SI_QUEUE`. The shared kill funnel was refactored into
`kill_common_value` / `tgkill_common_value` / `sys_signal_send_with_info(args, si_code,
value)` so all gate ordering (EFAULT → forging-EPERM → ESRCH-before-EINVAL → authority)
is shared and only the final post stamps the payload. Linux's `do_rt_sigqueueinfo`
forging gate (`(si_code >= 0 || si_code == SI_TKILL) && caller != target → EPERM`) is now
enforced on all three queued-signal entry points; the recorded `si_pid`/`si_uid` is the
*real caller* (faithful + unforgeable), only `si_value`/`si_code` come from the user.
Verified ring-3 by `/bin/sigqueue` (`sigqueue(getpid(), SIGUSR1, {.sival_int=0x12345678})`
→ handler reads `si_code==SI_QUEUE(-1)`, `si_value.sival_int==0x12345678`,
`si_pid==getpid()`, printing `SLATE_GLIBC_SIGQUEUE_OK signo=10 code=-1 value=0x12345678
self=1`, boot test PASSED) plus in-kernel forging-gate (EPERM) and SI_QUEUE-bypass
(ESRCH-before-EINVAL) assertions.

### TD28. Linux `munmap` is 16 KiB-frame-granular (delegates to native handler), not 4 KiB-page-granular — FIXED 2026-06-16

**Where:** `kernel/src/syscall/linux.rs` — `sys_munmap` delegates to the native
`kernel/src/syscall/handlers.rs::sys_munmap`.

**What it is:** the native `munmap` requires a **16 KiB-frame-aligned** start
(`vaddr.is_multiple_of(FRAME_SIZE)`, else `BadAlignment` → `EINVAL`), rounds the
length **up** to a whole 16 KiB frame, unmaps at whole-frame granularity, and
removes only a VMA that *starts exactly* at `vaddr` (`pcb::remove_vma`, not the
`remove_vma_range` surgery). Linux `munmap(2)` on x86-64 accepts any **4 KiB
(page)**-aligned start and unmaps an arbitrary page-granular sub-range, splitting
VMAs at 4 KiB boundaries. So three behaviours diverge from Linux:
1. A 4 KiB-aligned-but-not-16-KiB-aligned start returns `EINVAL` where Linux
   succeeds.
2. A length that is a multiple of 4 KiB but not 16 KiB is rounded **up**, so the
   unmap can spill 4 KiB sub-pages into an adjacent mapping that shares the
   straddling 16 KiB frame.
3. A partial unmap that does not start on a VMA boundary drops no VMA record
   (leaves a stale `[start,end)` VMA), where Linux would split it.

**Why it is not currently biting:** every base address our `mmap` hands back is
16 KiB-aligned (we allocate whole frames), and glibc only `munmap`s regions it
received from `mmap`, so in practice the start is always 16 KiB-aligned and
adjacent glibc mappings are themselves 16 KiB-aligned — the round-up does not
cross into a live neighbour. The Path-Z real-glibc tests (hello/stdio/full/
pthread) all pass with the current handler.

**Proper fix:** give the Linux `sys_munmap` its own 4 KiB-granular path, parallel
to the 4 KiB-granular `sys_mmap`/`sys_mprotect` work: validate `HW_PAGE_SIZE`
(4 KiB) alignment, unmap each 4 KiB sub-page PTE via an `unmap_4k` primitive
(refcount-aware `frame::free_frame` only when the last sub-page of a 16 KiB frame
is unmapped), and call `pcb::remove_vma_range(pid, start, end)` (already 4 KiB-
capable — it splits at arbitrary boundaries) for the VMA surgery, refunding
`RLIMIT_AS` for the actual span. Blocked only by the per-sub-page frame-refcount
bookkeeping (deciding when a shared 16 KiB frame's last 4 KiB tenant leaves).

**Fix (2026-06-16):** `sys_munmap` (`kernel/src/syscall/linux.rs`) now has its own
4 KiB-granular path and no longer delegates to the native handler. It (1) gates
exactly like Linux `do_vmi_munmap` — unaligned (to 4 KiB) start → `EINVAL`; a
length that rounds to zero (incl. `len == 0`) → `EINVAL`; address-arithmetic
overflow or a range leaving user space → `EINVAL` (Linux surfaces all of these as
`EINVAL`, **not** `ENOMEM`); (2) tears down each 4 KiB sub-page PTE via the
existing refcount-aware [`unmap_user_range`] primitive (frees the backing 16 KiB
frame only once its last sub-page tenant is gone, so a partial unmap sharing a
straddling frame with a live neighbour leaves the neighbour intact); (3) performs
4 KiB-boundary VMA surgery via `pcb::remove_vma_range` (splits the covering
VMA(s), retaining/releasing file-backing references for the surviving/removed
pieces); and (4) refunds `RLIMIT_AS` for the bytes of VMAs that *actually*
overlapped `[addr, end)` (computed before the surgery via `linux_vma_overlap_bytes`,
so a never-mapped or VMA-less range refunds 0 — matching that eagerly-mapped PIE
segments were never charged to `linux_as_bytes`). The per-sub-page refcount
bookkeeping that "blocked" this was already solved by `unmap_user_range` (written
for the `MAP_FIXED` overlay path), so no new frame-accounting code was needed.
Verified by an in-kernel gate self-test (`linux.rs` batch 533b: 4 KiB-unaligned
start → EINVAL; 4 KiB-aligned-but-not-16-KiB start no longer EINVAL — reaches pid
resolution → ESRCH from the boot task, proving the alignment is now accepted with
no side effect; out-of-range → EINVAL) plus a clean Path-Z boot-test (BOOT_OK,
0 self-test failures).

**Related fix (2026-06-15):** `remove_vma_range`'s **right** remainder
`[end, vma.end)` previously kept the original `FileBacked.file_offset` while its
`start` moved forward from `vma.start` to `end`, so the surviving high-side piece
of a split file-backed VMA mapped the wrong bytes. Now built via `vma_subrange`
(which advances `file_offset` by `end - vma.start`), matching the `protect_vma_range`
surgery. The left remainder was already correct (its start is unchanged).

### TD27. `mprotect` updates PTE permissions but not VMA flags — a reclaimed-then-refaulted RELRO page restores the old (writable) permission — FIXED 2026-06-15

**Where:** `kernel/src/syscall/linux.rs` — `sys_mprotect`; the VMA surgery lives in
`proc::pcb::protect_vma_range` (with `vma_subrange` for boundary splitting and
`vma_coverage_gaps` for the hole/ENOMEM check). The demand-fault resolver that
reconstructs a PTE from the covering VMA's `flags` is `pcb::try_resolve_fault` /
`pcb::resolve_subpaged_fault`.

**What it was:** `mprotect(2)` changed the live page-table entries for the range
but did **not** split/adjust the underlying `Vma.flags`. As long as the page stayed
resident this was invisible, but if a page in the range was later reclaimed under
memory pressure (`madvise(MADV_DONTNEED)`, or a future swap/anon reclaim path) and
re-faulted, the fault resolver rebuilt the PTE from the *VMA's* stale `flags` — so a
page glibc made read-only for RELRO would come back **writable**, silently weakening
the hardening. There was also a *correctness* bug for demand-paged mappings: glibc's
pthread thread-stack path `mmap(PROT_NONE)` then `mprotect(…, RW)` *before first
touch*, so a PTE-only mprotect left the not-yet-faulted region with its stale
PROT_NONE protection and the worker thread's stack writes faulted — surfacing as
`pthread_create` → EINVAL.

**Fix (2026-06-15):** `sys_mprotect` now calls `pcb::protect_vma_range`, which
performs per-subpage VMA surgery — it splits the covering VMA(s) at the (4 KiB-
aligned) range boundaries via `vma_subrange` (adjusting `FileBacked.file_offset`
and dup'ing backing references for the extra pieces) and recomputes
`WRITABLE`/`NO_EXECUTE` on `Vma.flags` for the affected sub-range, so the fault
resolver reconstructs the correct permissions after reclaim *and* freshly-mmapped
demand-paged regions fault in with the post-mprotect protection. Coverage (Linux's
"ENOMEM on a genuine hole") is checked before any mutation via `vma_coverage_gaps`
combined with a present-PTE check, so the eagerly-mapped (VMA-less but PTE-present)
PIE main-executable segments that glibc RELRO-protects are accepted while true holes
still return ENOMEM. Verified by the Path-Z real-glibc pthread self-test
(`proc::spawn::self_test_linux_real_glibc_pthread`: 4 threads via clone+TLS, 40000
mutex/futex ops, pthread_join) reaching `SLATE_GLIBC_PTHREAD_OK` and exit 13.

### TD26. User-mode CET shadow-stack state (`IA32_PL3_SSP`, `IA32_U_CET`) will be the next instance of the F13/F14 bug class when user CET is enabled — FORWARD-LOOKING HAZARD 2026-06-14

**Where:** `kernel/src/cet.rs` — `set_user_cet(enable_shstk, enable_ibt, user_ssp)`
and `read_user_ssp()`, both currently `#[allow(dead_code)]`. The per-task
context-switch save/restore lives in `kernel/src/sched/mod.rs` (the two
switch sites near lines 3779/3795 and 3974/3985 that already restore
`IA32_FS_BASE` and `IA32_GS_BASE`).

**What it is:** a forward-looking hazard, not a live bug. User-mode CET
(shadow stacks / IBT) is **not currently wired up for user tasks** — the
shadow-stack MSRs `IA32_PL3_SSP` (per-thread user SSP) and `IA32_U_CET`
(per-thread user CET config) are written only by the dead-code
`set_user_cet`, which nothing calls. So today there is no per-thread CET
state to clobber. The doc comment on `set_user_cet` already *claims* it is
"Called during context switch to restore per-task CET state" — that wiring
does not yet exist.

**Why it matters:** `IA32_PL3_SSP` and `IA32_U_CET` are exactly the same
**bug class** as F13/F14 (FS/GS base): they are userspace-settable
*per-thread* CPU register state that lives in MSRs, **not** in the saved GP
`Context` and **not** in the XSAVE area unless XSAVES + the CET_U state
component (bit 11) is enabled. The moment user shadow stacks are turned on,
each thread gets its own shadow stack and its own SSP; if the SSP (and the
U_CET enables) are not saved on switch-out and restored on switch-in, the
first context switch will leave a thread running on another thread's shadow
stack → spurious `#CP` faults or a security hole (shadow-stack reuse). This
audit (the same sweep that found F13/F14) flagged it proactively so it is
not re-discovered the hard way.

**Proper fix (when user CET is enabled):**
1. Add `pub user_ssp: u64` and `pub user_cet: u64` fields to `Task`
   (`kernel/src/sched/task.rs`), symmetric to `fs_base`/`gs_base`; `0` =
   no user CET (the default).
2. In both `sched::mod.rs` switch sites, after the FS/GS restore, restore
   `IA32_PL3_SSP` and `IA32_U_CET` for user tasks (gated on the task
   actually having CET enabled, to avoid a `#GP` writing an SSP MSR when
   CET is off in CR4/U_CET).
3. Sync the fields wherever the SSP/U_CET change: thread creation (allocate
   the shadow stack), `clone`/`fork` (new thread gets a fresh shadow stack;
   `fork` child inherits the parent's SSP value but its own COW shadow-stack
   page), and `exec` (reset to a fresh shadow stack or `0`).
4. Alternatively, if XSAVES is adopted, enabling the CET_U state component
   (XCR0/IA32_XSS bit 11) folds SSP/U_CET into the existing
   `xsave64`/`xrstor64` context-switch path — preferable because it reuses
   the FPU save machinery instead of hand-rolled MSR save/restore. Decide
   between explicit MSR save and XSAVES-CET_U at the time user CET lands.

**Trigger:** do this in the same change that first calls `set_user_cet`
from a live path (i.e. when user-mode shadow stacks / IBT are enabled for
user processes). Until then this is inert dead code and there is nothing to
fix.

### TD24. `link`/`linkat` return a blanket `EROFS` regardless of mount/filesystem — RESOLVED 2026-06-16 (Path Z Part 28)

**Resolution (2026-06-16, commit 5c8ae3e77 "Wire link/linkat to the VFS"):**
this is no longer accurate. `link`/`linkat` now do real VFS work for ring-3
callers: `link_common` (`kernel/src/syscall/linux.rs`) resolves oldpath/newpath
against the caller's cwd/dirfds via `resolve_at_path`, requires a File-WRITE
capability, and calls `Vfs::link`. ext4 implements real hard links (the Part 28
self-test creates one on the `/mnt` ext4 mount and reads it back); memfs cannot
share an inode between two names, so it correctly reports unsupported (mapped to
the filesystem-appropriate errno, matching Linux's `EPERM` for an FS without a
`->link` op — not the misleading `EROFS` this entry was filed against). Only the
kernel-context path (`caller_pid().is_none()`, no fd table) still returns the
`EROFS` terminal, which is required to keep the batch-481 syscall-fidelity
self-test green. The two residual fidelity gaps — `Vfs::link` always follows a
symlink oldpath (so plain `link(2)`'s no-follow contract and `linkat` without
`AT_SYMLINK_FOLLOW` are not honoured for the rare symlink-oldpath case) and
memfs lacking hard-link support (an inode-table refactor) — are tracked under
**B-SYM1**, not here. The historical analysis below is retained for context.

**Where (historical):** `sys_link` / `sys_linkat` in `kernel/src/syscall/linux.rs` (both
return `errno::EROFS` after validating their path/flags arguments).

**What it is:** no filesystem in the OS implements hard links, so both syscalls
fail unconditionally with `EROFS` ("read-only file system"). Linux instead
returns errno by case, in `do_linkat`/`vfs_link` order: oldpath missing →
`ENOENT`; newpath already exists → `EEXIST`; the two paths are on different
mounts → `EXDEV`; the destination mount is read-only → `EROFS`; and a writable
filesystem that simply lacks a `->link` op → `EPERM`. The common real case —
`link("/tmp/a", "/tmp/b")` on our *writable* `/tmp` memfs — should be `EPERM`
(unsupported), not `EROFS` (which misleadingly claims the mount is read-only).

**Related sub-fix landed 2026-06-14 (directory `st_nlink`):** memfs previously
hardcoded every node's `st_nlink` to `1`, including directories. A Unix
directory's link count is `2` (its name in the parent + its own `.`) plus one
per immediate subdirectory (each subdir's `..`); files/symlinks do not bump it.
`find(1)`'s leaf optimisation keys off `nlink == 2` (no subdirs ⇒ skip stat'ing
entries), so the hardcoded `1` both defeated that optimisation and reported a
count no real filesystem produces. memfs now computes directory link counts
honestly via `MemFsNode::nlink_count()` (files/symlinks still report `1` because
file hard links remain unimplemented — the main debt below). This does NOT
resolve TD24: `link`/`linkat` still return blanket `EROFS`.

**Why it's not a live bug today:** programs that use `link(2)` for speed
(git's `link_or_copy`, rsync `--link-dest`, `cp -l`, `ln`) fall back to copying
or report the error; none branch on `EROFS`-vs-`EPERM` in a way that corrupts
data. The only observable effect is a misleading error *message* on an
operation that cannot succeed regardless.

**Proper fix:** the real fix is hard-link support in the backing filesystems
(a substantial FS feature — memfs/ext4/FAT inode link-count + dirent aliasing).
Until then, an interim accuracy improvement would resolve oldpath/newpath, emit
`ENOENT`/`EEXIST`/`EXDEV` (the `KernelError::CrossDevice` variant added 2026-06-14
already maps to `EXDEV`) / `EROFS` / `EPERM` in Linux's order. That interim step
was deliberately NOT taken: faithfully reproducing `do_linkat`'s lookup ordering
(`AT_SYMLINK_FOLLOW` oldpath resolution, `AT_EMPTY_PATH`, dirfd resolution,
parent `ENOTDIR`/trailing-slash handling) for a syscall that always fails risks
introducing *new* divergences that are worse than the current honest-but-coarse
`EROFS`. Revisit when hard links are actually implemented.

### TD16. epoll fd readiness not reported when an epoll is nested in poll/select/epoll — RESOLVED 2026-06-14

**Where:** `kernel/src/ipc/epoll.rs` + the `HandleKind::Epoll` arm of
`poll_revents_from_entry` in `kernel/src/syscall/linux.rs`.

**What it was:** an epoll fd is itself pollable on Linux (it reports `EPOLLIN`
when any monitored fd is ready), allowing epoll fds to be nested inside another
epoll/poll/select. The `HandleKind::Epoll` arm of `poll_revents_from_entry`
returned 0 (never-ready), so nested-epoll readiness was NOT reported. `epoll_wait`
over directly-monitored fds always worked fully; only the nested case was wrong.

**Resolved (2026-06-14):** added `epoll_instance_ready(pid, handle, depth)` next
to `poll_revents_from_entry`. The Epoll arm now, given the threaded `owner_pid`,
resolves the epoll's `interest_list` against that process's fd table and reports
`POLLIN|POLLRDNORM` if any member is ready. Non-epoll members are evaluated by
`poll_revents_from_entry` (which never recurses back, as only the epoll arm calls
the helper); nested-epoll members recurse into `epoll_instance_ready` with
`depth + 1`, bounded by `EP_MAX_NESTS = 4` (mirrors `fs/eventpoll.c`) so a cyclic
or pathologically-deep nest can never blow the kernel stack. Without an
`owner_pid` (kernel/self-test context) the arm still reports not-ready rather
than consult an unrelated process's fd table. Boot self-test added in
`syscall::linux::self_test` ("nested-epoll readiness (TD16) OK"): a throwaway
process with a pipe → inner epoll E1 (watches pipe read) → outer epoll E0
(watches E1), asserting both E1 and the nested E0 are not-ready on an empty pipe,
both ready after a write, and not-ready when evaluated with `owner_pid = None`.

### TD15. timerfd `TFD_TIMER_CANCEL_ON_SET` is a silent no-op — RESOLVED 2026-06-14

**Where:** `kernel/src/timekeeping.rs` (generation counter), `kernel/src/ipc/timerfd.rs`
(stamp/check/wake), `kernel/src/syscall/linux.rs` (`sys_timerfd_settime`,
`dispatch_timerfd_read`), `kernel/src/syscall/handlers.rs` (`sys_clock_settime`,
`sys_clock_adjtime`).

**What it was:** `timerfd_settime` accepted the `TFD_TIMER_CANCEL_ON_SET` flag
(bit 1) without error, but the cancel-on-clock-step behavior was NOT implemented.
On Linux, a `CLOCK_REALTIME` timerfd armed with an absolute expiry and this flag is
"cancelled" (read returns `ECANCELED`, poll reports `POLLIN` readiness — *not*
`POLLERR`, contrary to the original note here) if the system realtime clock is
discontinuously changed (settimeofday/clock_settime/NTP step).

**Fix (implemented):** `timekeeping` now keeps a `REALTIME_GENERATION` counter,
bumped on every discontinuous realtime-clock step (`set_realtime`,
`adjust_realtime`); a smooth TSC advance does NOT bump it. `sys_timerfd_settime`
honours `TFD_TIMER_CANCEL_ON_SET` only for an absolute `CLOCK_REALTIME` timer,
snapshotting the generation into the timerfd at arm time (`armed_gen`). On read,
`take_cancellation` / `BlockingRead::Cancelled` return `ECANCELED` once per step
(resyncing `armed_gen`); on poll, `is_readable` reports readiness while the
generation is stale (level-triggered, no explicit poll wake needed). A blocked
reader is woken promptly by `clock_was_set()`, called from the `clock_settime` /
`clock_adjtime` handlers after the step. Boot self-test added to
`timerfd::self_test` ("TFD_TIMER_CANCEL_ON_SET (TD15): OK"): arms an absolute
`CLOCK_REALTIME` cancel-on-set timer far in the future, steps the clock via
`adjust_realtime(0)` (bumps the generation without moving the clock value),
asserts the timer becomes readable / `take_cancellation` returns true exactly
once, and that a re-armed timer *without* the flag is unaffected by a step.

### TD14. Per-process CPU-time / fault / ctxsw accounting — RESOLVED 2026-06-13 (time + page-fault + context-switch counters all done)

**Where:** `kernel/src/syscall/linux.rs` `sys_getrusage` and `sys_times`;
`kernel/src/sched/task.rs` (`Task::user_ticks`/`sys_ticks`, `tick_burst(from_user)`);
`kernel/src/sched/mod.rs` (`timer_tick(from_user)`, `cpu_ticks(tid)`, `TaskInfo`);
`kernel/src/proc/thread.rs` (`process_cpu_ticks(pid)`, `process_fault_counts(pid)`,
`on_thread_exit`); `kernel/src/proc/pcb.rs` (`Process::{acct_,child_}{user,sys}_ticks`
and `{acct_,child_}{min,maj}_flt`, `ThreadExitAccounting`, `remove_thread`,
`try_reap`/`try_reap_any`, `process_acct_ticks`/`process_child_ticks`,
`process_acct_faults`/`process_child_faults`); `kernel/src/sched/mod.rs`
(`account_fault`/`fault_counts`, `ctxsw_counts`, `SwitchKind` threaded through
`schedule_inner`); `kernel/src/idt.rs` (`account_fault` calls in
`handle_page_fault`); `kernel/src/apic.rs` (CPL sampling in `handle_timer_irq`);
`kernel/src/fs/procfs.rs` (`build_pid_stat`, `build_pid_status` ctxsw lines).

**Resolved — base (2026-06-13):** Linux-style tick-sampling CPU-time
accounting. On every timer IRQ, `handle_timer_irq` reads the interrupted frame's
CPL (`(frame.cs & 0x3) == 0x3` ⇒ ring-3) and passes `from_user` down through
`sched::timer_tick` → `Task::tick_burst`, which charges the whole tick to
`user_ticks` or `sys_ticks` (O(1), zero syscall-fastpath cost — Linux's default
non-NO_HZ_FULL model). `sched::cpu_ticks(tid)` exposes the per-thread split.

**Resolved — exited-thread fold + children-time (2026-06-13):** added a
per-process CPU-time accumulator to the PCB. When a thread exits,
`on_thread_exit` captures its `(user, sys)` ticks (while the Task is still
alive in the scheduler) and `remove_thread` folds them into
`Process::acct_user_ticks`/`acct_sys_ticks`. `process_cpu_ticks` now returns
`accumulator + Σ(live thread ticks)`, so it is exact for multi-threaded
processes that have already reaped worker threads — not just single-threaded
ones. For children time, `try_reap`/`try_reap_any` credit the parent's
`child_user_ticks`/`child_sys_ticks` with the reaped child's total CPU time
plus the child's own children-time (POSIX cutime/cstime carry-up, mirroring
Linux `wait_task_zombie` → `signal->cutime`/`cstime`). Both reset to 0 on fork.

Wired into:
- `getrusage(RUSAGE_SELF)` → process roll-up (live + exited threads);
  `getrusage(RUSAGE_THREAD)` → current thread; `getrusage(RUSAGE_CHILDREN)` →
  children accumulator. `ru_utime`/`ru_stime` populated (ticks×10ms → timeval).
- `times(2)` `tms_utime`/`tms_stime` and `tms_cutime`/`tms_cstime`
  (USER_HZ==TICK_RATE_HZ==100, so tick counts map directly to clock_t).
- `/proc/<pid>/stat` fields 14/15 (utime/stime) and 16/17 (cutime/cstime).

Self-test: `pcb::test_cpu_time_accounting` exercises the exited-thread fold,
`process_cpu_ticks` after all threads exit, and the parent←child←grandchild
children-time carry-up (asserts parent sees `(5+2, 3+1)`). Boot-test PASSED.

**Resolved — page-fault counters (2026-06-13):** added per-task `min_flt`/`maj_flt`
to `Task` (sched/task.rs) charged by `sched::account_fault(tid, major)` from the
three user-fault resolution points in `idt.rs::handle_page_fault` — swap-in ⇒
major (required I/O); demand-page (CoW/demand-zero) and stack growth ⇒ minor.
Mirroring the CPU-time path, the PCB gained `acct_min_flt`/`acct_maj_flt`
(exited-thread fold) and `child_min_flt`/`child_maj_flt` (reaped-children
carry-up). `remove_thread`'s signature was refactored from positional tick args
to a `ThreadExitAccounting { user_ticks, sys_ticks, min_flt, maj_flt }` struct
(the proper fix vs. a 6-arg signature). `proc::thread::process_fault_counts(pid)`
sums live + exited; `pcb::process_child_faults(pid)` reports the children
accumulator. Wired into `getrusage` `ru_minflt`(off 64)/`ru_majflt`(off 72) for
SELF/THREAD/CHILDREN, and `/proc/<pid>/stat` fields 10/11/12/13
(minflt/cminflt/majflt/cmajflt). `test_cpu_time_accounting` extended to assert
the fault fold (grandchild `(3,1)`), child children-faults `(3,1)`, and parent
children-faults `(4+3, 2+1) = (7,3)`. Boot-test PASSED.

**Resolved — context-switch counters (2026-06-13):** added per-task
`nvcsw`/`nivcsw` to `Task`, charged at the scheduler switch point. A
`SwitchKind` enum (`Voluntary`/`Involuntary`/`Uncounted`) is threaded into
`schedule_inner` from its five call sites (`yield_now`/`block_current`/
self-`suspend` ⇒ voluntary; `preempt` ⇒ involuntary; `task_exit` ⇒ uncounted)
and the outgoing task's counter is bumped under the SCHED lock at the actual
switch (where `next_id != current_id`). The PCB gained
`acct_nvcsw`/`acct_nivcsw` (exited-thread fold) and `child_nvcsw`/`child_nivcsw`
(reaped-children carry-up); `ThreadExitAccounting` carries the two fields too.
`proc::thread::process_ctxsw_counts(pid)` sums live + exited;
`pcb::process_child_ctxsw(pid)` reports the children accumulator. Wired into
`getrusage` `ru_nvcsw`(off 128)/`ru_nivcsw`(off 136) for SELF/THREAD/CHILDREN,
and `/proc/<pid>/status` `voluntary_ctxt_switches`/`nonvoluntary_ctxt_switches`
(previously stubbed as `0`/`schedule_count`). `test_cpu_time_accounting`
extended to assert the ctxsw fold (grandchild `(6,4)`), child children-ctxsw
`(6,4)`, and parent children-ctxsw `(7+6, 5+4) = (13,9)`. Boot-test PASSED.

**TD14 is now fully resolved** — all `getrusage` time/fault/ctxsw fields, `times`,
and the `/proc/<pid>/stat` + `/proc/<pid>/status` accounting surfaces are sourced
from real per-task counters rolled up per process with children carry-up. The
only rusage fields left at 0 are ones Linux also commonly leaves 0 (`ru_ixrss`,
`ru_idrss`, `ru_isrss`, `ru_nswap`, `ru_msgsnd`/`msgrcv`, `ru_nsignals`,
`ru_inblock`/`oublock`), which would require swap-RSS integral / signal-IPC
accounting not yet modelled.

### TD13. A few Linux-compat-flavored fields live in the native PCB — WATCH 2026-06-13

**Where:** `kernel/src/proc/pcb.rs` — job-control stop state
(`ProcessState::Stopped`/stop-signal tracking) and the `PR_SET_PDEATHSIG`
parent-death-signal storage (`get`/`set` around lines 2282–2290; field noted
"not wired because we don't yet have user-signal infrastructure").

**What it is:** the native process control block carries a small amount of
state whose *origin* is Linux/POSIX semantics (job-control stop/continue and
`prctl(PR_SET_PDEATHSIG)`). Per design-decisions.md §4 and §12, Linux-ABI
constructs should stay confined to the compat layer / Linux-ABI PCB state and
not accrete in the native PCB.

**Why it's not a live bug:** stop/continue is arguably a general
process-lifecycle notion (not strictly Linux), and `PR_SET_PDEATHSIG` storage
is inert (delivery is unwired). Nothing native consumes these as signals;
native process control remains IPC-based and faults remain SEH-style
exceptions. So the native ABI is not actually leaking *behavior* today.

**Proper fix (when the boundary is next touched):** move the pdeathsig value
(and any other purely-Linux fields) into the Linux-ABI PCB side-state (next to
`KernelFdTable`/the saved auxv), keyed by pid, so the native PCB carries only
constructs that would exist if Linux had never existed. Keep `ProcessState`
lifecycle states that are genuinely ABI-neutral. The trigger to do this is the
Linux compat ELF loader / signal-infrastructure work landing — co-locate all
Linux-ABI per-process state there in one pass rather than piecemeal.

### TD12. DRM event `read(2)` returns EAGAIN instead of blocking when empty — DEBT 2026-06-13

**Where:** `dispatch_drm_card_read` in `kernel/src/syscall/linux.rs`.

**What it is:** `read(2)` on a `/dev/dri/cardN` fd drains queued KMS events
(flip-complete records from `PAGE_FLIP` with `DRM_MODE_PAGE_FLIP_EVENT`).
When the event queue is empty it returns `EAGAIN` unconditionally — it does
not honour a *blocking* fd by parking the caller until an event arrives
(unlike, e.g., the signalfd read path, which has a real wait queue).

**Why it's not a live bug today:** our DRM backends retire page flips
**synchronously** inside `DrmDevice::page_flip`, so a flip-complete event is
queued *before* the `PAGE_FLIP` ioctl returns. A client following the normal
pattern (submit flip with the EVENT flag, `poll(2)` the fd, then `read(2)`)
always finds the event already queued; `poll` reports `POLLIN` immediately
and the read succeeds. The empty-read path is only reachable by a client
that reads without having submitted a flip — a client bug — and returning
EAGAIN there prevents a kernel hang rather than causing one.

**Proper fix (deferred until a backend retires flips asynchronously):** add a
per-client DRM event wait queue (mirroring the signalfd waiter pattern:
`register` + re-check + `block_current`, woken by `queue_event`), and have a
blocking read park on it instead of returning EAGAIN. Only worth doing once
a real vblank/async-flip source exists; under synchronous retirement it is
dead code.

### TD11. DRM dumb-buffer mmap not ref-tracked across `fork()` — DEBT 2026-06-13

**Where:** `drm_mmap_dumb` in `kernel/src/syscall/linux.rs` (the
`HandleKind::DrmCard` mmap interception in `sys_mmap`), in concert with
the refcounted `mm/frame.rs::free_frame` and the process-exit teardown
in `mm/page_table.rs::clear_user_address_space`.

**What it is:** The DRM Linux-ABI shim's MAP_DUMB path maps a dumb
buffer's GEM frames into the calling process by `frame::ref_inc`-ing
each frame before `map_frame`, so process-exit teardown's refcounted
`free_frame` merely balances the extra ref rather than double-freeing
the buffer (the GEM object retains its own ref until `gem_destroy`).
This is correct for a single process. It is NOT correct under a future
deep-copying `fork()`: a child that inherits the user PTEs for a dumb
mmap does not get a second `ref_inc`, so if fork ever gains general
per-page CoW of arbitrary user VMAs, a dumb mmap inherited by a child
and torn down on both sides could mis-count the frame refcount.

**Why it's not a live bug today:** our `fork()` does not deep-copy
arbitrary user mappings (see todo.txt Judgment Calls, fork(), 2026-05-31),
and graphics clients are single-process and do not fork while holding a
live framebuffer mmap. The gap is unreachable in practice.

**Proper fix (deferred until fork does general user-VMA copying):**
teach the fork path to recognise DRM-dumb-backed VMAs (or, more
generally, externally-refcounted frames) and `ref_inc` each frame per
child mapping, so every address space that maps a frame holds exactly
one ref and teardown stays balanced. Also recorded in todo.txt under
Judgment Calls.

### TD10. ALSA PCM shim does not implement the STATUS ioctl — RESOLVED 2026-07-15

**RESOLUTION (2026-07-15):** `STATUS` and `STATUS_EXT` are now implemented.
The ABI-target ambiguity that had deferred this (below) is resolved by the
obviously-correct choice for a new 64-bit OS: **target the time64 variant**
(64-bit `time_t` → 16-byte `struct timespec`), which is what a modern 64-bit
ALSA-lib is compiled against. `kernel/src/audio_alsa.rs` gains a byte-exact
`SndPcmStatus` (`size_of == 152`, asserted in `self_test`), from which the
request numbers derive: `SNDRV_PCM_IOCTL_STATUS == 0x8098_4120`
(`_IOR('A',0x20,152)`) and `SNDRV_PCM_IOCTL_STATUS_EXT == 0xC098_4124`
(`_IOWR('A',0x24,152)`), both asserted. `alsa_pcm_ioctl_status`
(`kernel/src/syscall/linux.rs`) fills `state`/`appl_ptr`/`hw_ptr`/`delay`
(= queued frames, what `snd_pcm_delay(3)` reports) / `avail` (playback: free
buffer space `buffer_frames − delay`; capture: full buffer) from the same
`sync_position` snapshot as SYNC_PTR, plus monotonic reference timestamps
(`clock_monotonic`) and the `trigger_tstamp` stamped at `START`. The ring
buffer size is captured at `HW_PARAMS` (`audio_alsa::buffer_size_frames` reads
the client-committed `BUFFER_SIZE` interval → `alsa_pcm::set_buffer_size`).
`avail_max` reports the current `avail` (a truthful lower bound — we don't
track a running peak); `overrange` is 0 (capture is synthesised silence).
Boot-validated: `[alsa] ALSA PCM ABI self-test PASSED` (struct size + ioctl
encodings) and `[alsa_pcm] PCM instance lifecycle self-test PASSED` (delay=2 /
avail=1022 / buffer_frames=1024 / trigger-stamped-on-start). Design note in
`design-decisions.md` (time64 ALSA ABI target). Original entry preserved
below for context.

---

### TD10 (original). ALSA PCM shim does not implement the STATUS ioctl — DEBT 2026-06-13 (narrowed 2026-06-13)

**Update (commit 4b):** SYNC_PTR and READI_FRAMES are now implemented.
`alsa_pcm_ioctl` (`kernel/src/syscall/linux.rs`) stores `boundary` /
`avail_min` from SW_PARAMS, computes `appl_ptr` (= frames submitted) and
`hw_ptr` (= `appl_ptr − mixer-buffered frames`) reduced modulo the
boundary, and answers `SNDRV_PCM_IOCTL_SYNC_PTR` with a byte-exact
`snd_pcm_sync_ptr` (the status/control pages sit in 64-byte unions, so the
payload size is independent of the timestamp ABI). `READI_FRAMES` returns
zeroed capture frames. Both are covered by the
`ipc::alsa_pcm::self_test()` boot self-test (SYNC_PTR snapshot appl=2/hw=0,
appl_ptr/avail_min push-adopt, capture silence read).

**What still remains:** `alsa_pcm_ioctl` returns **ENOTTY** for
`SNDRV_PCM_IOCTL_STATUS` / `STATUS_EXT`.

**Why STATUS is still deferred:** unlike SYNC_PTR, the `snd_pcm_status`
payload embeds bare `struct timespec`s directly (not inside a padded
union), so its `sizeof` — and therefore the ioctl request number — depends
on the time64-vs-legacy-timespec ABI (the ambiguity flagged in the
commit-2 note at the top of `todo.txt`). Pinning that layout down is a
self-contained follow-up. STATUS is also only a convenience overlay: a
conforming ALSA-lib client learns `hw_ptr`/`appl_ptr` from SYNC_PTR (now
handled), so STATUS-on-ENOTTY does not block the playback hot path.

**Empirical confirmation of the fork (2026-06-14):** the upstream
`struct snd_pcm_status` declares its trailing pad as
`unsigned char reserved[64 - 5*sizeof(struct timespec) - 5*sizeof(int)]`
(older kernels: `reserved[52 - 4*sizeof(struct timespec)]`). With a
**16-byte** 64-bit `struct timespec` that pad size goes **negative**,
which cannot compile — proof that the mainline kernel never uses a single
struct with a 64-bit timespec here. Instead it maintains **two distinct
ABI structs**: a legacy `snd_pcm_status` built on a 32-bit
`old_timespec32` (used by the `SNDRV_PCM_IOCTL_STATUS`/`STATUS_EXT`
request numbers compiled for a 32-bit timespec) and a separate
`snd_pcm_status` / time64 path (`__SNDRV_PCM_IOCTL_STATUS_EXT64` etc.)
built on `__kernel_timespec`. The two carry **different `_IOR` request
numbers** because their `sizeof` differs. Consequently we cannot just
"pin the timespec layout" — implementing STATUS means deciding *which*
alsa-lib variant our userspace targets and answering the matching request
number(s). Until that target is fixed, emitting one guessed number risks
silently mismatching the client's other variant. This is the concrete
reason STATUS stays deferred rather than being a quick add.

**Impact:** low. SYNC_PTR (the per-period pointer exchange ALSA-lib's
kernel plugin actually relies on) works; only the `snd_pcm_status()`
convenience query falls back to ENOTTY.

**Proper fix:** add byte-exact `snd_pcm_status` (resolving the timespec
layout against our 64-bit `struct timespec`), define
`SNDRV_PCM_IOCTL_STATUS` / `STATUS_EXT` from its `size_of`, fill it from
the same `sync_position` snapshot plus the trigger/reference timestamps
once a monotonic audio clock exists, and replace the ENOTTY arm.

**Related limitations (not debt, intentional first-cut scope):** the shim
advertises only `RW_INTERLEAVED` access (mmap-based clients unsupported)
and only the mixer's native 48 kHz / S16_LE / stereo format (non-native
configs are rejected by HW_PARAMS rather than resampled/converted).
Resampling + format conversion + an mmap transfer path are future work.

### TD9. Linux program interpreter (ld.so) + PIE executable loaded at a fixed base — no ASLR — RESOLVED 2026-06-14

**Resolution (PIE-executable base, 2026-06-14):** the main `ET_DYN`/PIE
executable base is now randomised too. A new `choose_exec_load_bias(is_pie)`
helper (`kernel/src/proc/spawn.rs`) returns `0` for `ET_EXEC` and, for PIE,
an ASLR base ≥ `LINUX_PIE_BASE` drawn via `apply_aslr_base(LINUX_PIE_BASE,
rng::next_bounded(PIE_ASLR_SPAN_PAGES))` (28 bits of entropy, 16 KiB-page
units, falling back to the fixed floor before the CSPRNG is seeded). It is
computed once per spawn/exec at the two `exec_load_bias` sites
(`spawn_process` + `exec_process`) and already threads uniformly through
`load_segments_with_bias`, the biased entry point, and the SysV stack
builder's `AT_ENTRY`/`AT_PHDR`, so the whole image relocates consistently.
The highest PIE base (`≈0x5955_5555_0000`) leaves ~22 TiB below the
interpreter floor (`0x7000_0000_0000`) for the image + brk growth, and the
PIE floor sits far above the mmap window (`0x60_0000_0000`), so no
collision is possible. `sys_brk` is now a real demand-paged heap (see the
"Linux brk(2) heap" resolution below): a PIE image's heap grows from its
page-aligned image end up to a ceiling of `LINUX_INTERP_BASE`, i.e. into
that 22 TiB headroom, and the grow path's VMA-overlap check is a second
guard against colliding with the interpreter or mmap window. Covered by
`spawn::self_test`'s
`test_pie_aslr_window` (alignment + ≥1 TiB interpreter-floor headroom).
Both halves of TD9 are now done; entropy/always-on policy is in
design-decisions.md #20.

**Resolution (interpreter base, 2026-06-14):** `load_interpreter` in
`kernel/src/proc/spawn.rs` now draws a per-exec randomised base from the
`LINUX_INTERP_BASE` window instead of using the fixed constant. A new pure
helper `apply_aslr_base(fixed_base, rand_pages)` adds `rand_pages *
FRAME_SIZE` (saturating) to the low edge; the page index is drawn unbiased
from `[0, 2^INTERP_ASLR_BITS)` via `rng::next_bounded`. `INTERP_ASLR_BITS =
28` mirrors Linux x86_64's default `mmap_rnd_bits` (28 bits of layout
entropy), applied in our 16 KiB page units → a 4 TiB window whose top
(`≈0x73FF_FFFF_C000`) stays far below `USER_STACK_GUARD`, so a randomised
base can never collide with the stack, the low-loaded executable, the brk
heap, or the general mmap window (`0x0060_…`); the interpreter image is the
window's sole occupant, so intra-window collisions are impossible too.
`AT_BASE` already carried whatever base was chosen, so ld.so relocation is
unaffected. Before the CSPRNG is seeded (very early boot, before any Linux
process can spawn in practice) it falls back to the fixed low edge.
Covered by `spawn::self_test`'s `test_apply_aslr_base` (alignment +
in-window + stack-clearance + saturation) and the existing
`self_test_linux_dynamic_interp` end-to-end launch (the test interpreter's
exit code is register-only/position-independent, so it runs correctly at
any randomised base; verified loading at e.g. 0x701e77808000, not the fixed
0x700000000000). The entropy-bits choice is recorded in
design-decisions.md.

---



**Resolution (interpreter base, 2026-06-14):** `load_interpreter` in
`kernel/src/proc/spawn.rs` now draws a per-exec randomised base from the
`LINUX_INTERP_BASE` window instead of using the fixed constant. A new pure
helper `apply_aslr_base(fixed_base, rand_pages)` adds `rand_pages *
FRAME_SIZE` (saturating) to the low edge; the page index is drawn unbiased
from `[0, 2^INTERP_ASLR_BITS)` via `rng::next_bounded`. `INTERP_ASLR_BITS =
28` mirrors Linux x86_64's default `mmap_rnd_bits` (28 bits of layout
entropy), applied in our 16 KiB page units → a 4 TiB window whose top
(`≈0x73FF_FFFF_C000`) stays far below `USER_STACK_GUARD`, so a randomised
base can never collide with the stack, the low-loaded executable, the brk
heap, or the general mmap window (`0x0060_…`); the interpreter image is the
window's sole occupant, so intra-window collisions are impossible too.
`AT_BASE` already carried whatever base was chosen, so ld.so relocation is
unaffected. Before the CSPRNG is seeded (very early boot, before any Linux
process can spawn in practice) it falls back to the fixed low edge.
Covered by `spawn::self_test`'s `test_apply_aslr_base` (alignment +
in-window + stack-clearance + saturation) and the existing
`self_test_linux_dynamic_interp` end-to-end launch (the test interpreter's
exit code is register-only/position-independent, so it runs correctly at
any randomised base). The entropy-bits choice is recorded in
design-decisions.md.

**What remains (PIE-executable base — still DEBT):** the position-independent
*main* executable is still loaded at the fixed `LINUX_PIE_BASE =
0x5555_5555_4000`. Randomising it is more delicate than the interpreter
because the brk heap grows immediately above the PIE image, so the PIE
ASLR window must be chosen to leave room for brk growth without colliding
with the mmap window below or the interpreter window above. Deferred as a
separate follow-up. Original debt write-up follows.

---



**What:** The Linux dynamic-linker load path (`load_interpreter` in
`kernel/src/proc/spawn.rs`) maps the program interpreter (ld.so) at a
**fixed** virtual base, `LINUX_INTERP_BASE = 0x0000_7000_0000_0000`,
every time.  Real Linux randomises the interpreter base (and the mmap
region generally) via ASLR.  The executable itself is also loaded at its
fixed link-time vaddr (PIE executables are not yet re-based either).

**Where:** `kernel/src/proc/spawn.rs` — the `LINUX_INTERP_BASE` constant
and `load_interpreter()`.  AT_BASE is reported correctly from whatever
base is chosen, so making this random is a localised change.

**Why it's debt, not a bug:** ASLR is a security hardening measure, not a
correctness requirement — ld.so relocates itself to wherever it is placed
using the base it is told (AT_BASE) and its own dynamic relocations.  A
fixed base is fully functional; it just removes the address-space
randomisation defence against exploitation.

**Proper fix:** Once a userspace mmap-region allocator / ASLR policy
exists, draw the interpreter base (and PIE executable base) from it with
per-exec randomisation instead of the fixed constant.  Keep the AT_BASE
plumbing as-is — it already carries whatever base is chosen.

**Update 2026-06-14:** the dependency is now in place — a per-process
VMA-aware mmap gap allocator (`pcb::reserve_unmapped_area` →
`mm::vma::find_gap`, fronted by `handlers::alloc_user_mmap_reserve`) now
serves the general user mmap window with freed-gap reuse and atomic
find+insert.  ld.so's general-region maps already flow through it; what
remains for TD9 is purely the *randomisation policy*: pick a randomised
base for the interpreter/PIE load instead of the fixed `LINUX_INTERP_BASE`
constant.  Note the interpreter is loaded at `0x7000_…`, disjoint from the
mmap window `0x0060_…`, so ASLR for it will need its own randomised
placement (or be folded into the mmap region) rather than just calling the
new allocator.

**Related limitation (not debt, just unimplemented):** end-to-end
interpreter *execution* is untested because no real glibc/musl ld.so is
on the filesystem yet.  The load mechanism (base selection, biased
segment mapping via `load_segments_with_bias`, AT_BASE/AT_ENTRY auxv) is
unit-tested via `spawn::test_load_interpreter_fallbacks` (static-ELF and
absent-interpreter `Ok(None)` fallbacks).  See `todo.txt` "Linux
dynamic-linker (ld.so) load path".

### TD25. `sys_brk` was a no-op stub (claimed grow succeeded but mapped nothing → latent SIGSEGV) — RESOLVED 2026-06-14

**What it was:** `sys_brk` (`kernel/src/syscall/linux.rs`) simply echoed
`args.arg0` back to the caller — claiming the requested program break was
granted while mapping **no** memory.  Any real glibc/musl program whose
`malloc` used the brk fast path (it does for small allocations until the
main arena is exhausted) would write into the "granted" heap and take an
immediate page fault on unmapped memory → ring-3 SIGSEGV.  The stub only
happened to be harmless because no glibc binary runs end-to-end yet; it
was a live trap waiting for the first one.

**Resolution (2026-06-14):** Implemented a real demand-paged brk heap.

- **PCB state:** added `brk_start` (heap floor) and `brk_current` (program
  break) to `Process` (`kernel/src/proc/pcb.rs`), inherited verbatim across
  `fork` (CoW heap clone) and reset on `exec` — recomputed from the new
  image's page-aligned end for Linux images (`elf::image_end`), cleared to
  `0` for native images (no Linux brk heap).  Accessors `set_brk_region` /
  `get_brk` / `set_brk_current`.
- **VMA:** new `VmaKind::Brk` (`kernel/src/mm/vma.rs`) — faults exactly like
  `Anonymous` (demand-paged, zero-filled); exists so `/proc/<pid>/maps`
  labels it `[heap]` and `sys_brk` can find/resize its own VMA.  The heap is
  a single `[brk_start, round_up(brk_current))` VMA.
- **sys_brk semantics (Linux-faithful):** `brk(0)` / `addr < brk_start`
  query (return unchanged break); grow maps the new span by replacing the
  heap VMA (demand-paged) and charges `RLIMIT_AS` for the full added virtual
  span up-front (committed-by-default — no overcommit); shrink unmaps+frees
  faulted frames via `unmap_user_range` and refunds the charge; same-top-
  frame moves touch nothing.  On **any** failure (RLIMIT_DATA, RLIMIT_AS,
  VMA collision, OOM, overflow) it returns the *unchanged* break — exactly
  what glibc's `__sbrk` expects so it falls back to mmap and reports ENOMEM
  itself.
- **Heap ceiling (image-dependent):** `brk_ceiling(brk_start)` returns
  `USER_MMAP_BASE` for a low-loaded ET_EXEC (`brk_start < USER_MMAP_BASE`)
  and `LINUX_INTERP_BASE` for a high-loaded PIE (`brk_start >=
  USER_MMAP_BASE`), so the heap can never grow into the mmap window, the
  interpreter window, or the stack.  The VMA-overlap check is a second
  guard.

**Tests:** `syscall::linux::self_test_brk_logic` (pure: `brk_round_up`
boundary/overflow cases + `brk_ceiling` ET_EXEC/PIE/ordering) and the
ring-3 end-to-end `proc::spawn::self_test_linux_brk` (a real Linux-ABI
process queries its break, grows 32 KiB, writes a sentinel into the
*second* heap frame, reads it back, exits with that byte — exit `0x6D`
proves the grow + demand-paging of multiple frames works; both verified in
the boot-test serial log).

**Update (2026-06-14): `arch_randomize_brk` gap now implemented.** The heap
floor is the page-aligned image end shifted up by a random gap
(`spawn::choose_brk_start`), mirroring Linux x86_64's `arch_randomize_brk`
with 13 bits of entropy (matching Linux's position count per the
entropy-is-the-metric policy of design-decisions #20; 128 MiB max gap at our
16 KiB pages). Always-on when the CSPRNG is seeded, no-gap fallback before
seeding, "no heap" (`image_end == 0`) preserved. Covered by
`test_brk_aslr_gap` and exercised end-to-end by `self_test_linux_brk`. No
remaining gaps on the brk heap.

### TD8. `membarrier` PRIVATE_EXPEDITED issue without prior REGISTER returns 0 where Linux returns `-EPERM` — RESOLVED 2026-06-14

**What it was:** `sys_membarrier()` (`kernel/src/syscall/linux.rs`) accepted every
issue command (`MEMBARRIER_CMD_PRIVATE_EXPEDITED`,
`…_PRIVATE_EXPEDITED_SYNC_CORE`, `…_PRIVATE_EXPEDITED_RSEQ`) and returned 0
unconditionally. Linux v6.6's `membarrier_private_expedited()` first checks the
issuing mm's `membarrier_state` and returns **`-EPERM`** when the matching
`MEMBARRIER_STATE_*_READY` bit is not set — i.e. when the process never issued
the corresponding `…_REGISTER_*` command. That EPERM check runs **before** the
single-CPU `return 0` shortcut, so even on our uniprocessor an unregistered
`PRIVATE_EXPEDITED` issue should be `-EPERM`, not 0. Symmetrically, our
`…_REGISTER_*` commands were no-ops and `MEMBARRIER_CMD_GET_REGISTRATIONS`
always reported 0. (Note: `GLOBAL_EXPEDITED` *issue* is NOT gated on Linux —
only the three `PRIVATE_EXPEDITED*` issues are; the original note overstated
this.)

**Fix (implemented):** added a per-mm `membarrier_state: u32` READY bitmask to
`Process` (`kernel/src/proc/pcb.rs`), shared across the process's threads (so a
thread may register and a sibling issue), inherited verbatim across `fork`
(Linux's `dup_mm` memcpy) via `pcb::membarrier_register` / `membarrier_state`
accessors. `sys_membarrier` now resolves the issuing mm's state and routes
through the pure, unit-tested `membarrier_decide(cmd, state)`: `REGISTER_*` OR
in their READY bit; the three `PRIVATE_EXPEDITED*` issues return `-EPERM`
unless their bit is set; `GET_REGISTRATIONS` reports the registered-command
bitmask via `membarrier_registrations_mask`; `GLOBAL`/`GLOBAL_EXPEDITED` issue
need no registration. The boot self-test (`self_test_membarrier_registration`,
"membarrier per-mm registration gating (TD8): OK") exercises `membarrier_decide`
exhaustively and drives the per-mm READY-bit store (register/idempotency/
cross-command isolation/GET mask) through a throwaway `pcb::create` process —
solving the original "no owner mm at boot" testability blocker by testing the
pure helper and the pcb layer directly rather than through the syscall caller's
(absent) mm.

**Residual divergence — RESOLVED 2026-06-14:** Linux resets `membarrier_state`
to 0 on `execve` (`membarrier_exec_mmap`); we previously lacked an exec-time
PCB-reset hook (the same gap noted for `linux_dumpable`/`linux_keepcaps`/
`linux_thp_disable`), so a registration survived exec. Now fixed: added
`pcb::reset_linux_state_for_exec(pid)`, called from `spawn::exec_process` after
`reset_vmas_for_exec`, which clears (under one `PROCESS_TABLE` lock) exactly the
fields Linux unconditionally resets on every exec — `membarrier_state` → 0
(`exec_mmap`→`membarrier_exec_mmap`), `linux_dumpable` → 1 (`SUID_DUMP_USER`;
explicit `set_dumpable` in `begin_new_exec`), and the `linux_securebits`
`SECBIT_KEEP_CAPS` bit (bit 4 only — `cap_bprm_creds_from_file` clears it on
every exec, preserving the lock bit and every other securebit). That bit 4 is
now the **single source of truth** for `prctl(PR_SET_KEEPCAPS)` (see the
follow-up note below), so clearing it on exec resets keepcaps too. Fields Linux
preserves across a normal (non-privileged)
exec are left untouched: `linux_thp_disable` and `linux_memory_merge` (both
`MMF_INIT_MASK` mm-flags that the new mm inherits via
`mm->flags = current->mm->flags & MMF_INIT_MASK` — `begin_new_exec` has no
explicit THP/KSM override, so they survive exec), `linux_pdeathsig` (cleared
only on set-uid/caps exec, otherwise preserved per prctl(2)),
`linux_personality` (x86_64 `set_personality_64bit` only clears the unmodelled
`READ_IMPLIES_EXEC`; `ADDR_NO_RANDOMIZE` survives), `linux_no_new_privs`
(sticky), `linux_child_subreaper`, timer-slack. (An initial version of the hook
wrongly reset `linux_thp_disable`, repeating entry 98's mistaken "cleared on
execve" claim; corrected same session.) Self-test
`pcb::test_reset_linux_state_for_exec` asserts the cleared state (membarrier,
dumpable, keepcaps, securebits KEEP_CAPS bit with lock+other bits kept) and the
five preserved fields ("[proc]   exec Linux-state reset: OK"). The in-kernel
`membarrier` self-test
caller (no owner mm) keeps the "fence/0" behaviour by feeding `u32::MAX` to the
gating helper — there is no registration model for a kernel thread with no
sibling userspace threads.

**Follow-up — keepcaps/securebits single source of truth (2026-06-14):** the
exec-reset audit surfaced a real ABI incoherence: `prctl(PR_SET_KEEPCAPS)` was
backed by a standalone `linux_keepcaps` field while `SECBIT_KEEP_CAPS` lived in
`linux_securebits`, even though Linux stores both in the *same*
`cred->securebits` bit 4. `PR_SET_KEEPCAPS`/`PR_SET_SECUREBITS` wrote different
storage, so `PR_GET_KEEPCAPS` and `PR_GET_SECUREBITS` could disagree where Linux
keeps them identical. Fixed by removing the `linux_keepcaps` field and making
`pcb::get_keepcaps`/`set_keepcaps` thin views over `linux_securebits` bit 4
(set/clear only bit 4, leaving every other securebit intact). Also added the
missing Linux gate to the `PR_SET_KEEPCAPS` handler: once
`SECBIT_KEEP_CAPS_LOCKED` (bit 5) is engaged the flag is frozen and the call
returns `-EPERM` (`cap_task_prctl`, verified against torvalds/linux v6.6
`security/commoncap.c`). The gate is the pure helper
`keepcaps_change_allowed(securebits)` so it is unit-testable without a caller
PCB. Tests: `self_test_prctl_dispatch`'s keepcaps block now asserts get/set
coherence in both directions (keepcaps↔securebits bit 4) and the lock-gate
truth table; `pcb::test_reset_linux_state_for_exec` proves `set_keepcaps`
coherently drives bit 4 and the exec reset clears only it.

**Companion fix — PR_SET_SECUREBITS lock enforcement now unit-tested
(2026-06-14):** the same audit found the `PR_SET_SECUREBITS` lock-bit
enforcement (a set lock can't be cleared; a locked flag can't flip) was
inline in the handler and so its `-EPERM` path was unreachable from the
kernel-context boot self-test (no `caller_pid` PCB to seed locked bits) —
the test only covered value validation. Extracted the decision into the pure
`securebits_change_allowed(cur, new_val)` (mirrors `cap_task_prctl`) and added
a truth-table test to `self_test_prctl_dispatch` covering: no-locks→allowed,
new-lock→allowed, clear-set-lock→denied, flip-locked-flag (both
set→clear and clear→set)→denied, and locked-flag-kept-while-flipping-an-
unlocked-flag→allowed ("PR_SET_SECUREBITS lock-bit enforcement … : OK").

### TD7. `set_mempolicy_home_node` returns 0 where Linux returns `-ENOENT`/`-EOPNOTSUPP` — APPROXIMATION 2026-06-12

**What:** `sys_set_mempolicy_home_node()` (`kernel/src/syscall/linux.rs`)
returns 0 for any valid non-empty range. Linux v6.6 instead walks the VMAs
in `[start, end)` with `err` initialized to `-ENOENT`: it returns `-ENOENT`
when no VMA in the range carries an explicit `MPOL_BIND`/`MPOL_PREFERRED_MANY`
policy, and `-EOPNOTSUPP` for a VMA whose policy is some other mode. Only a
range that already has a bind/preferred-many policy yields 0.

**Why we diverge:** our `mbind` is a UMA no-op that does **not** store
per-VMA mempolicy, so the kernel cannot tell whether the caller previously
established a policy on the range. We pick 0 (the "policy was set, home node
applied" success outcome — the common real-world sequence where
`set_mempolicy_home_node` follows a successful `mbind(MPOL_BIND)`) over
`-ENOENT`. Returning `-ENOENT` would instead break that common path.

**Proper fix:** implement real per-VMA mempolicy storage so the VMA walk can
distinguish "no policy" (`-ENOENT`), "wrong policy" (`-EOPNOTSUPP`), and
"bind policy → apply home node" (0). Tracked as an open question
(`open-questions.md`) because the 0-vs-`-ENOENT` choice is a genuine
tradeoff. **Note:** batch 551 *did* fix the unambiguous part — the
`home_node` online check now runs before the len/end gates, matching v6.6.

### TD5. NUMA nodemask `{0, extra-node}` is rejected where Linux accepts it — APPROXIMATION 2026-06-12

**What:** `get_nodes_uma()` (`kernel/src/syscall/linux.rs`, used by
`sys_mbind` and `sys_set_mempolicy`) collapses Linux's full nodemask down
to two booleans — `mask_empty` and `mask_has_extra_bits` (any node other
than node 0 set) — and the callers reject `mask_has_extra_bits` with
`-EINVAL`. Linux instead **intersects** the user mask with
`current->mems_allowed` (= `{0}` on our single-node system) and checks the
*intersected* mask for emptiness in `mpol_ops[mode].create`.

**Divergence:** a mask of `{0, N}` (node 0 **plus** a non-existent node N)
is rejected by us (`-EINVAL`) but **accepted** by Linux for
`MPOL_PREFERRED` / `MPOL_BIND` / `MPOL_INTERLEAVE` / `MPOL_PREFERRED_MANY`,
because the intersection `{0,N} ∩ {0} = {0}` is non-empty. A mask of `{N}`
alone (no node 0) is `-EINVAL` in both (intersection empty), so only the
"node 0 present *and* an extra bogus node" case differs.

**Why it's an approximation, not a bug now:** real programs on a
single-node box pass either an empty mask or `{0}`; `{0, N>0}` is not a
shape `numactl`/libnuma/jemalloc/tcmalloc produce when only node 0 exists.
The result is also strictly *more* conservative (we reject something Linux
accepts; we never accept something Linux rejects).

**Proper fix:** have `get_nodes_uma` report the effective mask after
intersecting with `mems_allowed = {0}` (i.e. "is bit 0 set?") separately
from "are there bits we must hard-reject" (only bits above `MAX_NUMNODES`
are hard-rejected by Linux's `get_nodes` itself), and apply the per-mode
emptiness check to the *intersected* mask in `mpol_new_check`'s spirit.
This is only worth doing if/when we support more than one NUMA node.

### TD6. `move_pages` per-page node error stores `-EINVAL` where Linux stores `-ENODEV`/`-EACCES` — RESOLVED 2026-06-12 (batch 549)

**Resolution:** `sys_move_pages` now stores `-ENODEV` for any non-zero
target node, matching `do_pages_move`'s `err = -ENODEV` path (out-of-range
or `!node_state(node, N_MEMORY)`). On a single-node box every node but 0
lacks `N_MEMORY`, so `-ENODEV` is correct for all of them; the `-EACCES`
"valid node not in `task_nodes`" branch is unreachable when only node 0 has
memory. Batch-105 self-test Case 4 updated to expect `[0, -ENODEV, 0]`.
Original analysis retained below for reference.

---

**What:** `sys_move_pages` (`kernel/src/syscall/linux.rs`), in move mode
(`nodes != NULL`), writes `status[i] = -EINVAL` for any requested target
node other than 0 (we only have node 0). Linux's `do_pages_move`
(`mm/migrate.c`) instead validates each target node and stores a per-page
error via `store_status`: `-ENODEV` when the node is out of range or has no
memory (`!node_state(node, N_MEMORY)`), or `-EACCES` when the node is valid
but not in `task_nodes` (`!node_isset(node, task_nodes)`). On a single-node
box, target node 1 would be `-ENODEV` (node 1 has no `N_MEMORY`), not
`-EINVAL`.

**Divergence:** observable only in `status[i]` for a deliberately-bogus
target node; the syscall return code (0) is unaffected. Batch-105 self-test
Case 4 currently asserts `status == [0, -EINVAL, 0]`.

**Why deferred (not fixed in batch 548):** batch 548 fixed two
independently-verified divergences (missing pid→ESRCH lookup; invented
E2BIG cap) and intentionally did **not** guess at the per-page errno. The
exact `do_pages_move` node-validity path (range check → `N_MEMORY` →
`node_isset(task_nodes)` → `store_status`) needs its own verbatim v6.6
verification before changing the stored errno.

**Proper fix:** verify `do_pages_move`/`add_page_for_migration`/`store_status`
against v6.6, then store `-ENODEV` for out-of-range / no-memory nodes and
`-EACCES` for valid-but-disallowed nodes, and update Case 4's expectation.

### TD4. Monolithic `syscall::linux::self_test()` has an unbounded boot-stack frame — RESOLVED 2026-06-14

**Resolution (2026-06-14):** The split is complete. Every self-contained
validation block in `self_test()` is now wrapped in its own
`#[inline(never)]` nested helper (`fn self_test_NAME() -> KernelResult<()>`,
called via `?`), so each sub-frame is allocated and freed transiently around
its call and no single frame is the sum of all batches. The body went from one
monolithic ~1.4 MB frame to ~80 small per-block helpers. Three earlier helpers
that had grown to wrap multiple sibling blocks (`getrusage_sysinfo_times` = 5
blocks, `capget_capset` = 2, `sched_affinity` = 2) were peeled apart so each
block gets its own frame. A structural scan confirms **zero** bare top-level
blocks remain. The technique used throughout (Technique B): insert a 5-line
header — `self_test_NAME()?;` + `#[inline(never)] fn self_test_NAME() -> …
{ use crate::serial_println;` — immediately before the block's leading
comment, and a 2-line footer — `Ok(())` + `}` — immediately after the block's
closing brace; the block body is never reproduced or re-indented, so the wrap
is safe for arbitrarily large blocks. A non-inlined nested fn cannot capture
enclosing locals, which acts as a compile-time safety net against
mis-scoping. Every wrap was individually boot-tested (BOOT_OK) and committed.
This removes the F10 (`.bss`/`FPU_STRATEGY` silent-corruption) failure class
at its root rather than merely deferring it behind the boot-stack canary.

**Progress (2026-06-13):** Began the incremental `#[inline(never)]` split. The
two leading self-contained check groups were extracted into standalone
functions — `self_test_errno_mapping()` (errno round-trips + the `check_errno!`
macro, used nowhere else) and `self_test_native_translation()` (the
`linux_from_native` round-trips). Both are guaranteed behaviour-preserving:
their locals never escape the extracted region. `self_test()` now calls them
via `?`. This establishes the repeatable extraction pattern (cut a contiguous
region whose locals don't cross the boundary, lift to an `#[inline(never)] fn
… -> KernelResult<()>`, replace inline with a `?` call, build+boot-test).
Continue opportunistically: the safe cut points are regions that don't share a
reused local (e.g. the early checks share `args`/`r`, so a larger contiguous
run ending at the last use of those must be lifted as one unit). Remaining
work is the bulk of the ~40 k-line body.


**What:** `kernel/src/syscall/linux.rs::self_test()` is a single ~1.4 MB
function (~39 k lines, opens near line 35858, closes near line 75298) whose
body is one giant 4-space enclosing block. Each ABI-fidelity batch (536 and
counting) appends its own locals inside that block. In the unoptimized
debug build (`opt-level=0`, no LLVM stack-slot coloring), the compiler does
**not** reuse stack slots across the lexically-disjoint per-batch sub-blocks,
so the function's single frame is the *sum* of every batch's locals
(~480 KiB as of batch 536 and growing monotonically). It runs directly on
the guardless boot stack — this is exactly what caused F10 (silent
`.bss`/`FPU_STRATEGY` corruption when the frame overran the old 512 KiB
stack).

**Why it's debt, not a bug now:** F10's fix (2 MiB boot stack + 64 KiB
redzone canary) gives ~1000+ batches of runway and converts any future
overrun into a clean `FATAL: boot stack overflow detected` halt instead of
silent corruption. So the system is correct and self-diagnosing. But the
frame grows ~1 KiB/batch, so this only defers the wall; it does not remove
it.

**Proper fix:** split `self_test()` into many small `#[inline(never)]`
sub-functions (e.g. one per batch or per logical group, `fn self_test_b536()
-> Result<…>` …) called in sequence from a thin driver, so each sub-frame is
allocated and freed around its call and no single frame is large. This caps
the boot-stack frame regardless of batch count and is the real removal of
the F10 failure class.

**Why deferred:** the function is one giant 4-space block; a hand-split risks
silently mis-scoping locals shared across batch boundaries (a local defined
in an early batch and read in a later one would stop compiling, or worse,
shadow). Doing it safely means iterating in small chunks with a build after
each (~50 s/cycle), and the canary makes it non-urgent. **Trigger to do it
properly:** before the boot-stack usage (reported by the canary scan / a
future high-water mark print) crosses ~50 % of the 2 MiB stack, or
opportunistically when next touching the self-test scaffolding.

## B-KASAN-INSTRUMENTED-BUILD-PANICS-ON-ITS-OWN-REDZONE-CHECKS

**Status:** ✅ **FIXED 2026-08-12** — the flood and the panic are both gone.
Found 2026-08-12 by the first instrumented boot that got far enough to reach the
self-tests.

**Verification.** The instrumented boot of 2026-08-12 ran 5560 lines — well past
the self-tests that previously flooded and panicked — with **zero `[kasan]
CRITICAL` reports**. The 64-report cap is untouched at the point the hunt
window opens, which was consequence (1) below and the reason this blocked the
§107 escalation.

**That boot did not reach `BOOT_OK`**, but for an unrelated reason: it later
wedged mid-print on a page fault. That is tracked separately as
`B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT`; it is a different
failure (a deadlock, not a report flood) at a different place, and it is still
open. So the §107 escalation is unblocked *by this entry* but still gated on
that wedge.

**What the fix was.** Exactly the "proper fix" described below, implemented as
`kernel/src/mm/rawmem.rs` (`read_u8`/`write_u8`/`fill_u8` in inline `asm!`) with
every deliberate poisoned-memory touch in `heap.rs`/`poison.rs`/`quarantine.rs`
routed through it, plus walk 3 of `scripts/kasan-check-preshadow.py` as the
build gate, plus `rawmem::self_test()` at boot ahead of the poison/kasan/
quarantine self-tests it underpins. The design and the reasoning behind walk 3's
accessor-based violation rule are recorded in `design-decisions.md` §120.

`kasan_rt::self_test`'s `before` snapshot also moved to *after*
`self_test_freed_address()` returns, as described below. That was independently
worth doing: it is what made the assertion count setup traffic as the thing
under test, and it would have kept the measurement window fragile even with the
flood gone.

---

*Original report follows.*

**What happens.** The compiler-instrumented kernel (`scripts/kasan-build.sh`)
does **not** reach `BOOT_OK`. It runs almost the whole boot, then dies in the
`mm::kasan_rt` self-test:

```
[kasan] CRITICAL: out-of-bounds (heap redzone) on read of 1 bytes @ 0xffff80007eb05268 (shadow=0xfb)
   ... x64, then: [kasan] further reports suppressed after 64
[kasan] Self-test PASSED
[kasan-rt] Running self-test...
!!! KERNEL PANIC !!!
panicked at kernel\src\mm\kasan_rt.rs:386:13:
assertion `left == right` failed: kasan-rt: outlined check did not report
  left: 217
 right: 101
```

**Root cause — a third instance of the §118/§119 hazard.** The symbolized
backtrace of the flood is unambiguous:

```
#0 kernel::mm::kasan_rt::report
#1 __asan_load1_noabort
#2 core::ptr::read_volatile::<u8>        <-- instrumented
#3 kernel::mm::heap::check_redzone
#4 <mm::heap::KernelHeap as GlobalAlloc>::dealloc
#5 __rust_dealloc  #6 alloc::alloc::dealloc  #7 mm::kasan::self_test
```

`mm/heap.rs`, `mm/poison.rs`, `mm/quarantine.rs` and `mm/kasan.rs` **all**
carry the module-level `#![cfg_attr(kasan_instrumented, sanitize(address =
"off"))]`. It does not help them, for the reason already written down twice:
a module-level `sanitize` **cannot exempt a generic `core` function**, because
the monomorphisation is emitted into the *kernel* crate with the default
(instrumented) attribute. Every one of these modules does its actual byte
touching through `core::ptr::{read_volatile, write_volatile, write_bytes}` —
so the exemption is **cosmetic on exactly the operations that matter**.

These modules are the ones whose *entire job* is to read and write memory
KASAN has deliberately poisoned:

| Site | What it touches |
|---|---|
| `mm/heap.rs:138-141,305-308,1737-1740` | free-magic reads on a freed block |
| `mm/heap.rs:160-170` | free-magic + `FREE_POISON` writes |
| `mm/heap.rs:218,322` | redzone verify (`check_redzone`, the flood above) |
| `mm/poison.rs:138,157,174` | `poison_free`/`poison_alloc`/`poison_redzone` fills |
| `mm/quarantine.rs:145` | parking a freed slot poisoned — the hunt's core mechanism |

So the instrumented build reports a violation for every byte of every redzone
check and every poison fill. That is *correct* by ASan's rules and useless by
ours: the access is the detector, not the bug.

**Why the panic follows.** `kasan_rt::self_test` snapshots `report_count()`
into `before`, then calls `kasan::self_test_freed_address()`, which does a real
`alloc`/`dealloc` — and that `dealloc` runs `check_redzone`, generating ~116
reports of its own before the assertion is reached. `before + 1` is then wrong
by exactly that flood (217 vs 101). The snapshot is in the wrong place *and*
the flood should not exist; fixing the flood fixes both.

**Why this is worse than a failed boot.** Two consequences that would have
silently wrecked the hunt even if the assertion were relaxed:

1. **The 64-report cap is spent before the hunt window.** The self-tests run at
   reference line ~21662; the Path-Z checkpoint the hunt watches is at ~19579
   but the *armed* run poisons throughout. Once suppressed, a genuine
   B-KNULLJUMP report would never print.
2. **Armed, the flood is enormous.** This validation boot ran *without*
   `mm.corruption_hunt`, so KASAN was disabled (`ENABLED` defaults false) and
   only the self-test poisoned anything — stats `poisoned=112B,
   shadow_frames=3`. The reference armed boot reports `poisoned=133859680B,
   shadow_frames=197` with `total_parked=55372`. Armed *and* instrumented,
   every one of those parks and every redzone check on 133 MB reports. (Noting
   this because the tiny stats line looks alarming on its own and is not: it is
   the unarmed default, not a shadow-coverage failure.)

**Proper fix.** Add a small uninstrumented raw-access primitive — byte
load/store/fill implemented with `core::arch::asm!` — and route every
deliberate poisoned-memory touch in `heap.rs`/`poison.rs`/`quarantine.rs`
through it. `asm!` is the right tool rather than another `sanitize` attribute:
LLVM does not instrument inline-asm memory operands, so the guarantee holds
whether or not the helper is inlined, and it cannot be re-broken by someone
later calling a `core` generic from an "exempt" module. Then extend
`scripts/kasan-check-preshadow.py` with a **third root set** ("functions that
deliberately access poisoned memory must be uninstrumented"), so this class of
regression is caught by the build gate rather than by a 2.7-hour boot — the
existing gate walks only the pre-shadow root and the check-path root, which is
why it passed this binary. Separately, move `kasan_rt::self_test`'s `before`
snapshot to *after* `self_test_freed_address()` returns, so setup can never be
counted as the thing under test.

**Reproduce.** `./scripts/kasan-build.sh` then boot; ~2.7 h to reach the panic
(see Q43 on the cost). Partial evidence kept in `build/serial-test.txt` from
the 2026-08-12 run; symbol table for that binary in `build/kernsyms.txt`.

**Related.** `design-decisions.md` §118 and §119 (the same hazard from the
pre-shadow and check-path roots), `open-questions.md` Q43 (the profile's cost),
and `B-KNULLJUMP-SIGNAL` (what the profile was built to hunt).

---

### B-BENCH-RECORDER-CRASHED-FOR-FOUR-COMMITS-AND-THE-BOOT-GATE-PRINTED-PASSED — 2026-08-14 — ✅ FIXED 2026-08-14 (`scripts/bench-history.py`, `scripts/boot-test.sh`, `scripts/test-bench-history.py`)

**Symptom.** Every `--bench` boot from `368c128fd` onward wrote **no history
record at all**. The kernel measured correctly, the serial log was complete, the
tool printed its full comparison and a correct canary summary — and then died:

```
Traceback (most recent call last):
  File "scripts\bench-history.py", line 1233, in <module>
    sys.exit(main())
  File "scripts\bench-history.py", line 1222, in main
    record["canary_verdict"] = verdict
                               ^^^^^^^
NameError: name 'verdict' is not defined
=== Boot test PASSED ===
```

Note the last line. That is the whole bug.

**Cause 1 — the extraction took a binding with it.** `368c128fd` moved 55 lines
of canary-summary printing out of `main()` into `print_canary_summary()`, to make
the wording assertable from the test suite. The moved block began
`verdict = canary_verdict(canary)`, so the binding left with it — while `main()`
went on referencing `verdict` 250 lines further down. Python does not diagnose
this until the line runs.

**Why the refactor's own evidence could not see it.** That commit justified
itself with a measured behaviour-preservation check: assertions went 106 → 117,
none lost. That check was sound and remains true. It simply could not cover this,
because it can only cover functions that *have* tests, and `main()` had none —
the only code path in the tool that actually writes to `history.jsonl` was the
one path with no test. Extracting code out of an untested caller moves the
tested part and leaves the untested part holding a dangling reference; a test
suite that grows in the extracted half reports success either way.

**Cause 2, and the worse one — the boot gate discarded the exit status.**
`boot-test.sh` invoked the recorder as `python bench-history.py … || true`,
reasoning in a comment that "a missing python or a write failure must not turn a
healthy boot into a failed one". Both of those are true and both are already
handled elsewhere: python's absence by the `command -v` branch, a write failure
by the tool reporting it without exiting non-zero. What `|| true` actually
suppressed was the case nobody had in mind — the recorder *crashing*. So the
traceback scrolled past and `=== Boot test PASSED ===` was printed directly over
it, four commits running.

This is the project's recurring shape, one level up from where it usually
appears: **a check that cannot fail is indistinguishable from a check that
passes.** Here the check was the tool's own exit status, and it had been
explicitly disarmed.

**Cause 3, found by the new test on its first run.** `main()` finished by
printing `os.path.relpath(args.history, REPO_ROOT)`. On Windows `relpath` raises
`ValueError` when the two paths are on different drives, so any `--history`
outside the checkout's volume aborted the tool *after* the record had already
been appended — a traceback and a non-zero exit on a run that had in fact
succeeded. Cosmetic path-prettifying must not be able to fail the run it reports
on.

**Fix.**

* `print_canary_summary` returns its verdict on all four paths, and `main()`
  takes the value from it rather than recomputing `canary_verdict(canary)`. The
  coupling is now explicit: there is no way to use the printer's output without
  receiving the value `main()` needs, and no second call site where the printed
  prose and the stored verdict could drift apart.
* `display_path()` falls back to the path as given when no relative form exists.
* `boot-test.sh` captures the recorder's status into `BENCH_RECORDER_STATUS`.
  This invocation passes no `--fail-on-regression`, so the tool has no legitimate
  non-zero exit here — any non-zero status is a fault in the tooling.
* New `finish_pass()` is the **only** place that prints the PASSED banner. The
  two success paths (the poll loop spotting the marker; the post-loop check
  finding it after QEMU exited) each carried their own verbatim copy of the
  pass sequence, so a condition added to one would silently not apply to the
  other. A run whose recorder failed now ends `=== Boot test INCOMPLETE ===`
  with **exit 3** — distinct from 1 (kernel/self-test failure) and 2 (wedge),
  because conflating "the kernel is broken" with "our tooling is broken" sends
  the reader to the wrong tree.
* `boot-test.sh`'s header now documents exit codes 2 and 3. 2 has existed since
  the stall detector landed while the header still claimed only 0 and 1 were
  possible; a status a caller cannot know about is a status the caller cannot
  handle.
* `test_main_records_end_to_end` drives `main()` to an appended record and
  asserts the record's contents, both canary paths, and all four of the
  printer's return values.

**Positive controls, because a regression test that cannot fail is the same bug
again.** Re-deleting the `verdict =` assignment reproduces the identical
`NameError` through the new test. Driving the real `finish_pass()` text with a
recorder stubbed to crash yields exit 3 and no PASSED banner; with a healthy
stub it yields exit 0 and PASSED. One further control was needed on the test
itself: the four return-path assertions were originally written *inside* the
`redirect_stdout` block that silences the printer, which posted their own
PASS/FAIL lines into the discarded buffer — four assertions running and
reporting nothing. They now collect under the redirect and assert outside it.

**Cost.** Four commits of `--bench` boots produced no data: roughly 9 minutes of
QEMU each, and — more expensively — the P21 baseline measured during this thread
had to be re-measured, because the run that produced it recorded nothing.

---

#### CORRECTION to P21(b), written BEFORE the measurement — the clause is not gradeable

Registered wording:

> **P21(b)** — `vfs_stat_breakdown_prologue` drops by **less** than
> `vfs_stat_breakdown_ns` does in absolute cycles, because the prologue's own
> `normalize_path` still allocates and is untouched. *Falsified if the prologue
> drops by as much or more*, which would mean the two benchmarks are not
> measuring the nested quantities the breakdown claims they are.

Reading the code the two benchmarks actually call (`kernel/src/fs/vfs.rs:1555`):

```rust
pub(crate) fn resolve_prologue(path: &Path) -> KernelResult<PathBuf> {
    let ns_path = crate::ipc::namespace::resolve_path(path)?;   // == the _ns benchmark
    let path = ns_path.as_path();
    validate_path(path)?;
    Ok(normalize_path(path))
}
```

`prologue` **strictly contains** `ns`: it is `ns` plus `validate_path` plus
`normalize_path`. So if the `Cow` removes one allocation from inside
`namespace::resolve_path`, and A2 is untouched, both benchmarks lose *the same
absolute amount*. Equal absolute drops are what correct nesting **predicts**.

The registered clause has this exactly backwards. It calls the equal-drop case a
falsification and names "the two benchmarks are not measuring the nested
quantities" as the thing that case would demonstrate — when in fact an equal drop
is the *signature* of the nesting being right, and a prologue drop substantially
**smaller** than the `ns` drop would be the anomaly needing explanation.

What the wording was probably reaching for is the *percentage*: the same absolute
saving is a smaller fraction of the prologue's ~580 ns than of the `ns` phase's
~261 ns. That claim is true, but it is arithmetic, not a prediction — it follows
from the two baselines alone and cannot fail.

So P21(b) does not separate any outcomes. "Less" and "equal" are divided by a
boundary that run-to-run noise straddles, and both readings are consistent with
the code being correct. **It will be graded UNGRADEABLE, not confirmed and not
falsified** — and that grade is recorded here *before* the measuring boot
finishes, because a prediction rewritten after its number is known is worth
nothing.

The replacement, for the next time this decomposition is touched: the prologue's
absolute drop should land **within measurement noise of the `ns` phase's**
(baselines: 261/262 ns across two idle runs, so noise is well under 5 ns on that
phase). A prologue drop materially *smaller* than the `ns` drop would mean the
saving is being partly re-absorbed inside A2 — plausibly by allocator ordering,
since removing one allocation changes what state the next one meets — and that is
a real, falsifiable claim about a real mechanism.

**The lesson, which is the point of registering these at all:** P21(a) was
derived from a mechanism (an allocation on a no-op path) and is gradeable.
P21(b) was derived from a *feeling* that the nested benchmark ought to move
less, and the "because" clause attached to it was never checked against the four
lines of code it describes. A prediction whose stated falsification condition is
its own confirmation is the same defect this file keeps recording one level down:
**a check that cannot fire is indistinguishable from a check that passes.**

---

#### P16 instrument landed — first reading, and why it does NOT grade P16

`59d7cfc61` added the HPET-vs-TSC discriminator. Its first output, from the
release boot at `24a3407cc`:

```
[spawn]   sleep clocks: HPET 84338300 ns vs TSC/clock_realtime 84001888 ns across
          the child's lifetime (HPET/TSC = 1.00x)
[spawn]   -> AGREE within 5%: both oscillators saw the same interval ...
[spawn]   fastpy-on-SlateOS `sleep` ... : OK
```

**This run passed.** P16 is registered against "a boot where the child reports
< 40 ms", and this child reported well over it. So the reading is *not* evidence
for cause (1) and the "AGREE" line it printed must not be read as a verdict on
the bug — on a passing run the clocks agreeing is unremarkable, because nothing
was anomalous for them to disagree about. Banking it as a confirmation would be
the identical error recorded a few sections up for the P20 load run: reading a
result as confirming the hypothesis it happens to sit next to.

What it *does* establish is the **control arm**, which the instrument previously
had none of: on a healthy boot the two oscillators agree to within 0.4%
(84 338 300 vs 84 001 888 ns). That matters, because it rules out the boring
explanation in advance — the two clocks are not chronically skewed on this host,
so if a *failing* boot shows them diverging, the divergence is specific to the
failure rather than a standing property of the machine. Without this reading, a
1.36x ratio on a failing boot could not have been distinguished from a machine
where the ratio is always 1.36x.

P16 therefore remains **unresolved and awaiting a failing boot**. The test is
intermittent, so this is a matter of accumulating boots, not of doing anything
further to the instrument. Both branches are now reachable and both say which
subsystem to open.

Baselines for P21(a) now stand at three idle release runs — `vfs_stat_breakdown_ns`
= 262, 261, 262 ns — putting run-to-run noise on that phase under 0.5% and the
20% threshold far outside it. `vfs_stat_breakdown_prologue` = 580, 568 ns across
the two runs that recorded it (noise ~2%).
---

### [A] TOOLING-A-EDITING-A-SHELL-SCRIPT-WHILE-IT-IS-RUNNING-CAN-DERAIL-THE-RUNNING-COPY — ⚠️ HAZARD, avoided 2026-08-15

**Status:** not a bug in the repo; a standing hazard in how we work. Recorded
because it was very nearly hit today and the failure would have been baffling.

**The hazard.** `bash` does not read a script into memory and then run it. It
reads it *incrementally*, remembering a byte offset into the file. If the file
changes underneath a running shell, the shell resumes at its saved offset in
the **new** bytes — which no longer mean what they did. The result is not a
clean error: the shell executes whatever fragment now lives at that offset,
producing syntax errors from lines that are perfectly valid, or, much worse,
silently running a *different* command than the one on that line.

**How it nearly happened.** `./scripts/boot-test.sh --bench` was running in the
background (a ~17 min job). The next queued task was to add a free-space floor
to `scripts/boot-test.sh` — i.e. to rewrite, in place, the exact file a live
bash was mid-way through interpreting. Whether it corrupts depends on an
implementation detail nobody should have to reason about: an editor that writes
to a temp file and `rename()`s over the target is safe (the running shell keeps
the old inode), while one that truncates and rewrites in place is not. Our
editing tool's behaviour on Windows is not guaranteed to be the former.

**What makes it nasty.** The damage lands in the *long-running background job*,
not in the edit, so the two are separated by minutes and look unrelated. A
boot test that dies with a syntax error at line 900 of a script that `bash -n`
declares perfectly valid is a genuinely hard thing to diagnose, and the natural
first hypothesis — "the script is broken" — is wrong.

**Prescription.** Before editing any script, check whether a background task is
currently executing it, and wait. This is cheap: the queued edit loses a few
minutes; the alternative costs a confusing debugging session and a wasted run.
The same applies to `scripts/run-timeout.py` and any other harness file, and it
is *most* dangerous for exactly the files worth improving — the long-running
harnesses, which are running precisely when you have the idle window that
tempts you to improve them.

#### P16 control arm — second reading, now spanning both build profiles; and why today's two failing boots do NOT grade it

The debug boot at `6deaa847e` (2026-08-15) printed:

```
[spawn]   sleep clocks: HPET 119276000 ns vs TSC/clock_realtime 120088295 ns
          across the child's lifetime (HPET/TSC = 0.99x)
```

**This is a control reading, not a grading run** — the child reported 120 ms,
far above P16's < 40 ms trigger, so nothing anomalous existed for the clocks to
disagree about.

What it adds is that the control arm now spans **both build profiles**: 1.00x on
a release boot (84.3 ms child) and 0.99x on a debug boot (120.1 ms child). The
child's lifetime differs by 43% between the two — debug is slower, as expected —
and the two oscillators tracked each other through both. That is a stronger
statement than one profile could make: HPET-vs-TSC agreement is not an artefact
of a particular build's timing, so a divergence on a failing boot remains
attributable to the failure.

**Two boots failed today and neither one grades P16.** Break-tests #1 and #2
(deliberate mutations of `test_valid_entries` and `CapEntryInfo`) both died in
the capability self-test at serial line ~1071 — roughly 5 500 lines *before* the
spawn phase, so neither boot ever ran the sleep test at all. Counting them as
"failing boots" toward P16 would be precisely the error this entry already
records against the P20 load run: treating a failure as evidence about the
hypothesis it happens to sit near, rather than the one it actually exercises.
P16 needs a boot where **the sleep test itself** fails; a boot that failed
earlier for an unrelated, self-inflicted reason is not a sample of that
population.

P16 accordingly remains **unresolved and awaiting a qualifying failing boot**.

### [A] B-BENCH-A-PERSISTENT-REGRESSION-IS-REPORTED-ONCE-THEN-ABSORBED-INTO-ITS-OWN-RANGE — 2026-08-15 — 🔧 FIXED (harness defect fixed; both "regressions" it exposed are disproved — `http_build_response_1KiB` is a layout lottery per Finding 3, `vfs_stat_root` is smaller than one binary's own spread per Finding 4)

**Two findings: two genuine regressions, and the reason the harness stopped
reporting them on the very next run.**

#### The series (release runs, ns, oldest → newest)

```
                          e7b912d 0bd70ab 7a96b55 c43ce8a c43ce8a 8c3f844 8135d14 24a3407 f79aec5 c893184 e384f46 c5a4013
http_build_response_1KiB     6150    5992   10089    5964    6167    5990    5987    6018    5890  >8546  >12431  >12407
vfs_stat_root                3473    3777    3721    3453    3591    3635    3883    3998    3136    3278   >4488   >4429
vfs_stat_breakdown_full         -       -       -       -       -       -    3915    4015    3170    3219   >4424   >4505
ipc_eventfd                   541     653     652     544     540     539     660     641     534     537   1021     653
net_ipv6_parse                 81      81      80      80      81      79      80      80     113     169      80      80
```

#### Finding 1 — two regressions are real, one "regression" was noise

`http_build_response_1KiB` sat at ~6000 ns for nine runs, then went 8546 →
12431 → **12407**. `vfs_stat_root` sat in 3136-3998 for ten runs, then 4488 →
**4429**. `vfs_stat_breakdown_full` likewise: 3170-4015, then 4424 → **4505**.

The consecutive pairs agree to **0.2%** (12431 vs 12407) and **1.3%** (4488 vs
4429). That is the decisive point, and it survives the fact that *both* runs
were contaminated: host disturbance shows up as **stalls**, which inflate the
*mean*, and these figures are **min-of-N**. Noise does not reproduce to two
parts in a thousand. Two independently-disturbed runs landing on the same
number is evidence *for* a real shift, not against it.

By the same test `ipc_eventfd`'s spectacular +90% (537 → 1021) was **noise**: it
did not reproduce (653, back inside its long-standing bimodal 534-660). So the
earlier decision not to dismiss all three as contamination was right, and so was
declining to accept all three — one of the three was exactly what the
contamination story predicted, and two were not.

`net_ipv6_parse` is **resolved**: 80 ns across both runs, matching the nine runs
before the 113/169 excursion. The excursion is over and was never a code change.

#### Finding 2 — the harness reported "no benchmark moved outside its own range" on the run that confirmed the regressions

The re-run's verdict was:

```
No benchmark moved outside its own recent range (2 crossed 25% run-over-run).
```

Both statements are true and together they are misleading. The comparison is
**run-over-run against the immediately preceding run**, and "its own recent
range" is a window over the last 8 runs. So once a regression has appeared in
one run:

1. run-over-run sees 12431 → 12407 and correctly reports **no movement**; and
2. the range has absorbed the elevated sample, so 12407 is now **inside** it.

A regression is therefore visible for exactly **one run**. On the second run it
becomes the new normal, and the harness affirmatively reports the suite as
clean. This is worse than silence: a run that says "no benchmark moved outside
its own recent range" is naturally read as "no regressions", when what it
actually means is "nothing changed *since the regression*."

The window poisons itself, and it does so fastest for exactly the regressions
that matter most — a persistent one, which by definition appears in every
subsequent run.

**The proper fix** is for the range to be computed over runs that are *known
good*, not over "the last 8 whatever they were": compare against the median of
the last N runs **that predate the newest run's own value entering the window**,
or keep a pinned baseline per benchmark that only moves when a change is
explicitly accepted. A cheap interim check that would have caught this: flag any
benchmark whose newest value is >25% off the median of runs 5-12 back, in
addition to the existing run-over-run test.

**Not yet known: what caused the two regressions.** Both jumps bracket merges of
other lanes' work into `lane-a` as well as this lane's own `sys_cap_query`
change, so attribution needs a bisect over the recorded commits
(`f79aec5 → c893184 → e384f46`), not a guess. `http_build_response_1KiB` is the
better target: it more than doubled, in two clean ~45% steps, from a nine-run
plateau.

**Step 1 attribution, 2026-08-15: code layout is RULED OUT — and this is a
positive result, not an absence of evidence.** Both commits were built from a
scratch worktree (`build/straddle/kernel-f79aec561`, `kernel-c893184fa`) and
compared with `scripts/straddle-check.py --compare`. The pair is adjacent — one
commit, `mm/vfs: return Cow<'_, Path> from namespace::resolve_path` — which
touches nothing in the HTTP path.

The re-roll is enormous, exactly as the family analysis predicted:

| | count |
|---|---|
| loops that gained a straddle | 5181 |
| loops that lost one | 4997 |
| functions quarantined as recompiled | 9 |

The 9 recompiled functions are precisely `namespace::resolve_path`, its callers
and their self-tests — the signature check isolated the commit's real footprint
with no false positives, which is the first end-to-end evidence that the
quarantine works on a real pair rather than on synthetic inputs.

**And none of those 10178 flips is on this benchmark's path.** That is what
makes this decisive: the instrument fired ~10k times on the same binary pair
and still reported nothing here.

| on the `http_build_response_1KiB` path | loops | straddle change |
|---|---|---|
| `bench_build_response` (has `build_response` inlined) | none | — |
| `etag_for_body` — 103 B FNV loop over 1 KiB, **the dominant loop** | 2 | `no` → `no` |
| `memcpy` / `memset` / `memmove` | **none** (`rep movsb`, no backward branch) | — |
| `__rust_alloc` / `__rust_dealloc` | none | — |
| `core::fmt::write` | none | — |
| only httpd flip in the whole binary: `build_response_gzip` | 1 | straddle **lost** (faster, wrong direction) |

**The straight-line confound was measured too, not waved away.** `straddle-check`
models *loops*, but a TCG translation block is also cut by a page boundary in
straight-line code, so a call-heavy path could pay per call rather than per
iteration. Counting page-boundary crossings inside every function on the path
gives **2 in the old build, 3 in the new** — the single change being one integer
`Display` impl. The response formats two integers, so the worst case is two extra
dispatcher round-trips per call against a +2656-cycle regression. It does not
account for it.

This limitation of the tool is real and remains: `straddle-check` cannot see
straight-line page crossings, and here that had to be checked by hand. Logged so
the next comparison does not quietly assume loops are the only mechanism.

**Why the scratch worktree does not invalidate this.** The two binaries were
built in `os-straddle-scratch`, not in `os-lane-a` where the recorded bench runs
were built, and the two directory names differ in length — so if any absolute
build path were embedded, `.rodata` would shift and the scratch pair would be a
*different* draw of the layout lottery than the pair that produced the numbers.
Checked rather than assumed: the only absolute path in the image is
`D:\visual studio projects\os\netproto\src\ipv4.rs` and friends, which live
inside the **prebuilt service ELFs** that `include_bytes!` embeds. Those blobs
were built once on Aug 14 and copied in byte-for-byte, so they are identical in
both builds and do not vary with the kernel's build directory. No Cargo.toml
declares an absolute path dependency. The kernel therefore contains no string
that depends on which worktree built it, and the scratch pair reproduces the
lane-a pair.

A second, independent consistency check points the same way: the compare
quarantined exactly **9** recompiled functions, and they are precisely
`namespace::resolve_path`, its callers and their self-tests. Build
nondeterminism or a stale artifact would have produced signature changes
scattered far beyond one commit's source footprint.

**Tooling hazard found while checking this: `strings` is not installed on this
machine, and neither is `llvm-strings` in the rustup toolchain.** `strings -a
<elf> | grep …` therefore prints nothing at all — which is indistinguishable
from "the string is not in the binary", and is exactly the false negative that
would have "confirmed" path-independence for the wrong reason. It was caught
only because `grep -a -c` had already reported a match on the same file, so the
two disagreed. Use `grep -a -o -b ".\{0,60\}<pattern>.\{0,60\}" <elf>` instead;
it needs no extra tool and prints the byte offset and surrounding context.
`command -v strings` before trusting an empty result from it.

**So the regression has a non-layout cause, and the leading hypothesis is now
that the benchmark is not isolated from heap history.** The suite runs in a fixed
order in one address space; `build_response` allocates a `String` and grows a
`Vec`, so which slab/free-list path those hit depends on everything allocated
before them. A commit that removes allocations from path resolution changes the
heap's shape by the time the HTTP benchmark runs, without touching a line of
HTTP code. That is the same class of defect as the straddle lottery — a
whole-binary property masquerading as a per-benchmark regression — and it is
**not yet tested**; it is written down here as the next thing to falsify, not as
a finding.

#### Finding 2 — **FIXED 2026-08-15** (lane A): `level_shifts()` in `scripts/bench-history.py`

The harness defect is closed. A new check compares each run against a baseline
drawn from runs that **predate the shift** — `LEVEL_SHIFT_SKIP = 3` most-recent
runs are excluded from the reference window, so a regression introduced in the
last one-to-three runs cannot have entered its own baseline. This is the
"proper fix" option above (median of runs 5-12 back), not the cheap interim one.

On the recorded history it now prints, on the very run quoted above as reporting
the suite clean:

```
SUSTAINED SHIFT (>25% off a baseline from before the last 3 runs, and outside
that baseline's own spread -- these do NOT show up run-over-run once they persist):
  http_build_response_1KiB: was ~5991ns -> now 12407ns (+106% vs suite);
  pre-window baseline 5759-6277ns over 8 runs
```

**Two things had to be got right, and both were found by measurement, not
reasoning — recorded because the first version of each looked obviously
correct:**

1. **Persistence is the entire discriminator.** The first version compared only
   the newest run to the clean baseline. Replayed causally over all 26 recorded
   runs it fired on **11 (42%)**, including `net_ipv6_parse +110%` and
   `page_fault +103%` — the exact single-run excursions this same entry
   identifies above as noise. Drift correction does not save it: contamination
   is a **heavy tail, not a uniform slowdown**, so `speed_factor` (a median)
   removes the central shift and leaves the tail looking like several
   simultaneous regressions. Requiring the benchmark to be off-baseline in the
   newest run *and* the `LEVEL_SHIFT_PERSIST = 2` before it took this to 2/26,
   because host disturbance is random per run while a code regression is in
   every run after the commit.
2. **A flat percentage threshold is not scale-aware.** The one survivor was
   `ipc_channel_sync` (646 → 967 → 684 → **578** — i.e. noise), whose own 1.5-IQR
   fence is ~20% wide, so a 25% threshold sits *inside* its noise. Judging the
   level shift against Tukey's **extreme**-outlier fence (`k=3`) instead of the
   1.5 used elsewhere declines it while `http_build_response_1KiB` remains
   outside by a factor of two. Final rate: **1 firing in 26 runs**, the true
   positive.

**Known limitation, deliberate:** this inherits the flat 25% threshold, so the
concurrent `vfs_stat_root` shift (~3600 → ~4450, **+23%**) is below it and is
*not* reported by this check. That is the pre-existing blind spot, not a new
one; the `vfs_stat_root` regression above is still tracked by hand.

Regression tests: `scripts/test-bench-history.py`, three new cases. They were
**mutation-tested**, not merely observed to pass — deleting the persistence
check makes both the synthetic one-run-excursion case and the real-history
"stays quiet" control fail. Note for whoever tunes the constants: patching
`bh.LEVEL_SHIFT_PCT` at runtime does **nothing**, because `threshold_pct`
defaults to it and default arguments bind at definition time. A mutation test
that patches the module attribute silently tests nothing and reports success —
pass `threshold_pct=` explicitly, as the test does.

**Nothing is still open in this entry.** The harness defect is fixed;
`http_build_response_1KiB` is resolved by Finding 3 and `vfs_stat_root` by
Finding 4 — neither was a regression. **Read Findings 3 and 4 before acting on
Finding 1, which they supersede.**

#### Finding 3 (2026-08-15, supersedes Finding 1 for `http_build_response_1KiB`) — there is no regressing commit; the metric is **bimodal**, and the mode is a property of the binary

**In short:** I was bisecting for a commit that made this benchmark twice as
slow. There isn't one. The benchmark has two stable speeds — about 6000 ns and
about 10800 ns — and each *build* lands in one of them, essentially at random,
depending on where the compiler happened to place the code. Re-running the same
build always gives the same speed; changing almost any unrelated code can flip
it. The "regression" is the metric flipping into its slow mode, and it has
flipped **back and forth** several times already.

**The evidence.** Taking every release-profile, non-`loaded` record in
`bench/history.jsonl` (n = 20) and sorting by value gives a cleanly separated
pair of clusters with **nothing in between**:

| mode | n | mean | range |
|---|---|---|---|
| LOW  | 11 | 6055 ns | 5877 – 6396 |
| HIGH |  9 | 10806 ns | 8546 – 12934 |

The gap between the highest LOW (6396) and the lowest HIGH (8546) is empty.
**HIGH/LOW = 1.78×**, which is the documented TCG page-straddle penalty (~1.7×)
and not a number I chose.

**The mode is deterministic per binary — this is the decisive test.** Three
commits were measured more than once, seven measurements in total:

| commit | n | values (ns) | modes |
|---|---|---|---|
| `26c1c7330` | 3 | 12934, 8818, 11381 | HIGH only |
| `3f733c39c` | 2 | 9019, 11633 | HIGH only |
| `c43ce8acc` | 2 | 5964, 6167 | LOW only |

**Zero repeats cross the mode boundary.** Host noise moves a value by up to
1.47× *within* the HIGH mode (8818 → 12934) but has never once carried a HIGH
binary into LOW or the reverse. So the mode is a deterministic function of the
compiled image, while the scatter inside a mode is run-to-run noise. That is
exactly the layout-lottery signature: deterministic per binary, re-rolls
whenever unrelated code shifts an address.

**And the mode flips in both directions across the commit sequence:**

```
LOW LOW LOW | HIGH HIGH HIGH HIGH HIGH | LOW LOW | HIGH | LOW LOW LOW LOW LOW | HIGH HIGH HIGH
```

A regression caused by a bad commit does not un-regress and come back. This
sequence has five direction changes.

**What was wrong with Finding 1.** It read "sat at ~6000 ns for nine runs, then
went 8546 → 12431 → 12407" and concluded a step change. But its *own* series
table, printed directly above it, contains `7a96b55 → 10089` inside that
supposedly-flat stretch — a HIGH-mode reading that was set aside as noise
because it did not fit the step-change story. It is not noise; it is the same
mode the last three runs are in. The two-consecutive-runs-agree-to-0.2%
argument (12431 vs 12407) is still *true*, and still correctly rules out
run-to-run noise — but agreeing to 0.2% is precisely what two runs of the same
*mode* do. The argument distinguishes "not noise" from "noise"; it never
distinguished "code got slower" from "layout re-rolled", which was the actual
alternative.

**Consistency with the step-1 straddle falsification above.** No contradiction:
that analysis showed no *loop* straddle flip on the benchmark's path, and it
was right. It also found straight-line page crossings on the path changing
2 → 3, which at the time looked minor. Given the bimodality, straight-line
crossings are now the leading mechanism, and the loop-only tool blind spot
(since fixed) is why the first pass looked exculpatory.

**Consequences.**

- **The task "attribute the `http_build_response_1KiB` 2× regression" is closed
  with a negative answer.** There is no commit to attribute it to. Bisecting
  further is bisecting noise-with-structure and will keep producing
  plausible-looking but false attributions — `c893184fa` is *not* guilty, it
  merely re-rolled.
- **This metric must not gate anything until it is de-lotteried.** Any
  threshold between 6396 and 8546 fires on a coin flip. The harness's
  own-range check (`f0cb9eccf`) partly absorbs this, but only by widening the
  range until the metric says nothing at all.
- **`vfs_stat_root` is a different shape and is NOT explained by this** — but
  it is not a regression either; see Finding 4.

#### Finding 4 (2026-08-15) — `vfs_stat_root`'s "regression" is smaller than one binary's own run-to-run spread

**In short:** the other benchmark in this entry was also reported as regressing
(about 3600 → 4450). It isn't. A *single unchanged build* of this benchmark has
produced readings from 3344 to 5930 — a spread wider than the entire claimed
regression, which sits comfortably inside it.

`vfs_stat_root` is **not** mode-structured: its 21 release values form a
continuous 2623 – 6454 spread with no empty gap, and the repeat-commit test
declines it (commit `26c1c7330`'s own readings straddle every candidate split).
So the mechanism is different from Finding 3 — but the conclusion is the same,
for a simpler reason:

| commit | readings (ns) |
|---|---|
| `26c1c7330` | **5930, 4394, 3344** |
| `3f733c39c` | 2623, 3310 |
| `c43ce8acc` | 3453, 3591 |

One binary, `26c1c7330`, produced both 3344 and 5930 — a 1.77× spread with no
code change whatsoever. The reported regression is 3278 → 4488, and 4488 is
**below** one of that same binary's own readings. There is no effect here to
attribute: the metric's run-to-run noise is larger than the movement being
investigated.

`vfs_stat_breakdown_full` has only six release readings (3170 – 4505, 1.42×
spread) which is too few to judge, and it moves in step with `vfs_stat_root`;
absent any independent evidence it should be treated the same way until it has
enough history to say otherwise.

**Consequence.** The task "attribute the `vfs_stat_root` regression" is closed
with a negative answer, the same as Finding 3's. Both benchmarks are too noisy,
in different ways, to support the reports that were made about them — and in
both cases the disproof was already sitting in `bench/history.jsonl` and needed
no new boot. The general lesson is worth stating plainly: **before attributing a
movement to a commit, check what the same commit's own repeats do.** That is now
enforced automatically by `mode_structure()` in `scripts/bench-history.py`,
which reports a mode-structured shift as "NOT a regression to bisect" and
excludes it from `--fail-on-regression`.

**Method note, for reuse.** The test that settled this costs nothing and should
be the *first* step next time a benchmark "regresses": group the history by
commit, keep only commits measured more than once, and ask whether any of them
straddles the proposed threshold. If repeats never cross it, the metric is
mode-structured and bisection is the wrong tool. A great deal of straddle
tooling was built before anyone ran that three-line query — the raw data it
needed had been sitting in `bench/history.jsonl` the entire time.

### [A] B-BENCH-THE-ACCESS-FLOOR-CLAMP-BINDS-ON-EVERY-RUN-AND-SAYS-IT-MEASURED-SOMETHING — 2026-08-15 — 🔧 FIXED in `90457f629`; both constants re-derived per build profile (see RESOLVED section at the end of this entry)

**In short:** the benchmark suite calibrates two of its budgets against "how
much does one memory access cost on this machine", measures that as **5
cycles**, then quietly throws the measurement away and uses a hard-coded
**100** instead — on every run, without ever saying so. The line it prints
looks like a successful calibration and reads `measured=5.0 ... budgets below
are multiples of this`, where "this" is 100, not 5. So both budgets are 20x
looser than they claim to be, and they have never once been calibrated.

**Where:** `kernel/src/bench.rs`, `let floor = core::cmp::max(measured_cycles.unwrap_or(0), 100);`
(the `access_floor` binding, ~line 1432). Consumers: `mmio_suspicion =
access_floor * 4` (`fast_cpu_index` PASS/SLOW, ~line 1513) and `access_floor *
OWNER_TAG_BUDGET_ACCESSES` (frame-owner tagging, ~line 1658). It is also the
divisor in `accesses()`, which prints "N accesses" figures in the report.

**Evidence it binds every time.** Across every run in `build/` that used the
current 1024-stores/window calibration:

| run | measured | floor used |
|---|---|---|
| 4 runs, 1024 stores/window | 5.0, 5.0, 5.1, 5.1 cycles/store | 100 |

`max(5, 100)` is 100 in all four. There is no recorded run on the current
calibration where the clamp did not bind, so no budget verdict the suite has
ever printed was calibrated to the machine it ran on.

**Why the loud `UNMEASURED` path does not catch it.** That path fires only when
the measurement *fails* (arms did not separate, or the scale check rejected
it), and it says exactly the right thing: "falling back to the arbitrary clamp
... verdicts below are NOT calibrated". The case here is the opposite and worse
— the measurement **succeeded**, was believed, and was then discarded anyway by
a clamp whose own comment claims it exists only for the degenerate
`unwrap_or(0)` case. A clamp that binds on a good measurement is not a guard;
it is a silent override.

**The root cause is a units/quantity confusion, not the constant.** Two
different physical quantities are being asked of one variable:

1. **Cost of one memory access** (~5 cycles here). This is what is measured,
   and it is the right divisor for `accesses()` — "this delta is worth N memory
   accesses" is a meaningful sentence.
2. **The noise floor of a single-shot measurement** (~100-200 cycles here).
   This is what the *budgets* actually need, and the comment at the
   `mmio_suspicion` site says so in as many words: an absolute 200-cycle budget
   "reported SLOW on every healthy boot ... because 200 is below this harness's
   floor for a single memory access — the nop baseline alone wanders by more
   than that between adjacent measurements".

The 100 is a hand-tuned stand-in for quantity 2 wearing quantity 1's name. That
is why it cannot be simply deleted: dropping to the true 5 would make
`fast_cpu_index`'s budget 20 cycles, far below the measurement noise, and it
would report SLOW on every healthy boot — the exact bug the clamp was added to
paper over.

**Proper fix** (both halves, neither sufficient alone):

1. **Measure quantity 2 instead of hard-coding it.** The A/B already runs
   `CANARY_ROUNDS` interleaved rounds; the spread of the nop arm across those
   rounds *is* the single-shot noise floor, and it is free — it is already
   being computed and thrown away. Budgets become multiples of a measured
   dispersion, and the two quantities stop sharing a variable.
2. **Make the clamp announce itself.** Whenever the fallback is used at all —
   whether because the measurement failed or because it was overridden —
   the run must say so and its budget verdicts must be marked uncalibrated,
   the same treatment the `UNMEASURED` branch already gives. Three outcomes
   (measured / clamped / unmeasured), never two.

**Consequence of leaving it:** the two budget checks cannot fail on this
harness for any realistic regression, so they are decorative. Nothing else in
the suite depends on the floor, and the SCORE lines that gate `BENCH_OK` do
not, so this is a silently-dead check rather than a wrong number — which is
the failure mode this project keeps rediscovering: a check that cannot fire is
indistinguishable from a check that passes.

**Fix landed — `90457f629`.** Half 2 went in as written above: three outcomes
(`measured` / `CLAMPED` / `UNMEASURED`), and the CLAMPED branch states plainly
that the budgets are looser than the machine warrants, so a PASS is weak
evidence while a SLOW still counts.

Half 1 went in **differently from the proposal above, and the difference
matters**, so the reasoning is recorded here rather than lost:

- *Proposed:* measure quantity 2 directly, as the dispersion of the nop arm
  across the interleaved rounds — free, since it is already computed and
  discarded.
- *Implemented:* measure the cost of a **scattered** access —
  `measure_scattered_access_cost` walks a 512 KiB buffer at a 4 KiB stride, a
  distinct guest page per store, so each store pays its own softmmu lookup the
  way real allocator code does.
- *Why the change:* the proposal accepted the entry's own framing that the
  budgets want "the noise floor of a single-shot measurement". They do not.
  They want *the cost of the kind of access the code under test actually
  makes*, which is a physical property of the workload, not of the instrument.
  The nop-dispersion figure would have been an honest measurement of the wrong
  thing — a number that moves when the harness gets noisier and stays put when
  the memory system gets slower, which is backwards for a budget. The
  scattered cost satisfies the noise constraint too (it is one to two orders
  of magnitude above the hot cost, hence well clear of the ~200-cycle wander),
  so one honest measurement discharges both requirements instead of trading
  one confusion for another.
- The clamp is *retained* as the noise guard, because that is the one job the
  entry correctly identified for it — but it is now a floor that announces
  itself rather than a silent override, so a machine where the scattered cost
  genuinely came out under 100 cycles would say so instead of pretending.

The scattered measurement carries its own scale-invariance check, which
**halves** the store count rather than doubling it: doubling would run past
the end of the 512 KiB buffer and wrap onto already-resident pages, quietly
re-measuring the hot case and certifying it as scattered.

**Still open — the two budget constants.** `mmio_suspicion`'s `4` and
`OWNER_TAG_BUDGET_ACCESSES`'s `150` were both sized against the clamp, so each
has always meant a flat cycle count (400 and 15000) wearing an "N accesses"
label. Both are flagged `PENDING RE-DERIVATION` in the source. They are not
guessed at here because re-deriving them needs a boot that prints the measured
scattered floor, which had not run when this was written. The arithmetic is
already in the source comments and points the same way in both cases — the
`fast_cpu_index` comment says a healthy lookup "is one access or less" and
healthy boots measure 274-282 cycles, i.e. about *one* scattered access, not
four; the owner-tag comment reasons in "~16 accesses at TCG's
few-hundred-cycles-each" while the code divided by a 5-cycle one. Until they
are re-derived both budgets are merely loose, which is the direction that
cannot manufacture a false alarm.

#### RESOLVED 2026-08-15 — both constants re-derived, and the cause was a *profile* split, not a floor

The "pending re-derivation" above assumed the two budgets just needed
restating in the corrected (scattered) unit. That was the wrong diagnosis.
Re-deriving them from the recorded boots turned up something the unit story
does not explain:

| check | healthy **release** | healthy **debug** | old budget | old budget vs worst release |
|---|---|---|---|---|
| `fast_cpu_index` | 4-10 cycles (n=8) | 188-420 | 400 (`floor*4`) | **40x too loose** |
| `page_alloc_free_owner_ab` | 42-246 cycles (n=9) | 7660-12708 | 15000 (`floor*150`) | **61x too loose** |

The right-hand columns are the finding. **The 7660-11288 figures quoted in
the source comment as the healthy range were DEBUG boots**, and the comment
did not say so — the same kernel is ~40x slower in debug (`page_alloc_free`
is ~1330 cycles in release and ~52000 in debug). One constant was being asked
to span both profiles, so it was sized for debug and release lost: in release
neither check could fire at all. A check that cannot fire is indistinguishable
from a check that passes, which is why both had reported PASS forever.

**Fix:** both budgets are now absolute per-profile cycle counts selected on
`cfg!(debug_assertions)` — `mmio_suspicion` 100 release / 2000 debug,
owner-tag 1500 release / 40000 debug — and are no longer multiples of
`access_floor` at all. Each line also prints `[{} profile]` so a surprising
verdict can be attributed to the branch taken rather than to the code under
test. Verified on a release boot: `fast_cpu_index: PASS (8 cycles, limit 100
cycles [release profile])`, `page_alloc_free_owner_ab: PASS (92 cycles, limit
1500 cycles [release profile])`.

Note the second-order consequence: `access_floor` no longer feeds **any**
verdict. Its only remaining consumer is the display-only "N accesses" figures.

#### B-BENCH-THE-UNMEASURED-WARNING-VOIDS-VERDICTS-IT-NO-LONGER-GOVERNS — found and fixed in the same session

Caught by reading the verification boot's own log rather than its exit code.
With the budgets now absolute, the floor's failure message was still saying:

> Falling back to the arbitrary clamp of 100 cycles: budget-based verdicts
> below are NOT calibrated to this machine and **must not be read as
> findings**.

On that boot the scale check legitimately rejected the measurement, so this
printed — and the two lines immediately below it were sound absolute-cycle
verdicts that the reader was being told to discard. This is the exact mirror
image of the bug the entry above documents: **a warning that taints valid
findings trains the reader to ignore the instrument just as effectively as a
budget that cannot fire.** Both end in a verdict nobody acts on.

**Fix:** all three `memory_access_floor` messages (measured / CLAMPED /
UNMEASURED) now scope their claim to the "N accesses" figures, which are all
the floor still feeds, and state explicitly that the PASS/SLOW verdicts are
absolute per-profile cycle counts and still hold. The CLAMPED case also now
names the *direction* of its error (figures understated, because a bigger
divisor yields fewer accesses) instead of the previous vague "LOOSER than this
machine warrants".

**Generalisable:** when a consumer is removed from a shared calibration, the
calibration's *error messages* are part of its interface and go stale with it.
Grep the failure text, not just the call sites.
### A trailing `| tail` swallows the exit code too — and hides the log while it runs — 2026-08-15

Follow-up to "A trailing `tail` swallows the exit code the notification
reports". That entry says to make the command under test the **last** command in
the chain. That is not enough, because it is satisfied by:

```
python scripts/run-timeout.py 900 cargo test -p compositor ... 2>&1 | tail -50
```

`cargo` *is* the last command written, but a pipeline's exit status is the exit
status of its **last element**, so the notification reported `tail`'s 0. This
was reintroduced twice in one session by an agent who had written the original
entry the session before, which is the reason for restating it.

Two things are wrong with the pipe form and only one of them is the exit code:

1. **The status is `tail`'s.** The rule has to be stated as *the process whose
   status you care about must be the last element of the last pipeline* — not
   "the last command", which reads as satisfied by the above.
2. **The output file stays empty until the job ends.** `tail` cannot emit
   anything until its input closes, so the incremental log — the entire reason
   `run-timeout.py` streams and heartbeats — shows nothing while the job runs.
   Checking on a long build mid-flight returns an empty file, which reads as a
   hang.

Both vanish if the pipe is simply dropped. `run-timeout.py` already writes to
the harness's output file; use `Read`/`tail` on **that file** afterwards instead
of filtering in the pipeline. Reserve pipes for foreground commands whose status
does not matter.

---

# Lane B

*(none moved yet — see `requests/c-b-known-issues-archive.md`)*

---

# Lane C


## Byte-indexed display truncation panics on non-ASCII text (lane C)

**Status: FIXED 2026-08-15** (lane C, commits `f508f76cf`, `f53562a09`,
`feb695bbd`, `8208fad9d`, `83dfaff21`, `5750232c5`, `a8d659199`, `ffbdec410`,
`54fd94f2b`, `5305d139f`, `b3373ad17`, `db06a8c3c`, `de378bab6`, `37ee779ae`,
`10db32f9c`). Found while surveying app tables for unbounded columns. Eighteen
sites across `apps/` and `gui/` confused a byte count with a character count, usually
while truncating a *display* string:

```rust
let display = if title.len() > 20 {
    format!("{}...", &title[..17])   // panics if byte 17 is inside a character
} else {
    title
};
```

`str::len` is bytes and `&s[..17]` is a byte index, so any string whose 17th
byte falls inside a multi-byte character panics with
`byte index 17 is not a char boundary`. The guard makes it *more* likely, not
less: a 20-character Japanese title is 60 bytes, so it takes the truncating
branch and then slices mid-character. This is not an edge case for these
particular apps — it is their ordinary input.

| Site | String | Exposure |
|---|---|---|
| `apps/rssreader/src/main.rs:3256,3260` | `article.summary` / `display_content()` | **Remote.** Straight off an RSS feed; any non-English feed crashes the reader. |
| `apps/pdfviewer/src/main.rs:1452` | the PDF's own `/Title` | Attacker-supplied file metadata. |
| `gui/desktop/src/file_drop.rs:65` | dropped text | And our paths are byte strings by design. |
| `apps/flashcards/src/main.rs:1313,1370` | card front/back | A flashcard app is *the* place for CJK and accented text. |
| `apps/stickynotes/src/main.rs:973` | the note's first line | The user's own text. |
| `apps/procexplorer/src/main.rs:2359` | `KEY=value` from the environment | Environment strings are arbitrary bytes. |
| `gui/toolkit/src/colorpicker.rs:175` | `&s[..6]` on a typed hex string | Any multi-byte character in the field. |
| `gui/desktop/src/clipboard_viewer.rs:112` | `content[..197]` on a clipping | Copying any non-Latin text aborted the shell. |
| `gui/desktop/src/clipboard_viewer.rs:678` | `&preview_text[..40]` on the same | Same, one layer up. |
| `apps/videoplayer/src/main.rs:538` | `padded[..3]` in the SRT timestamp parser | **A subtitle file the user merely opened.** |
| `apps/renamer/src/main.rs:450,460,489,509` | the filename stem, cut at a position the user types | **Any non-ASCII filename**, and it aborts a batch rename *partway through*. |
| `apps/markdowneditor/src/main.rs` (14 sites) | `cursor_col`, the selection anchor, undo columns | **Press Down onto a line with a wide character, then type.** Aborts with the document unsaved. |
| `apps/backup/src/main.rs:302` | the `?` glob wildcard, over path bytes | **Not a panic** — an include/exclude pattern silently stops matching, so a file the user believed was covered is not backed up. |
| `apps/filesearch/src/main.rs` (both matchers) | every single-character construct in the glob *and* regex engines | **Not a panic** — a search over non-ASCII filenames silently returns wrong results, in both directions. |
| `apps/dbviewer/src/main.rs:895` | SQL `LIKE`'s `_` wildcard | `LIKE '_'` was false for a one-character CJK cell and `LIKE '___'` was true for it. |
| `apps/indexer/src/main.rs:709` | the `?` wildcard and `[...]` classes of a third glob matcher | Same as filesearch's, in the file indexer. |
| `apps/indexer/src/main.rs:826` | `levenshtein_bounded`, the fuzzy-match edit distance | One substituted kanji cost 3 of a budget the user reads as "a couple of typos", so near-exact CJK matches were rejected. |
| `apps/jsonviewer/src/main.rs:304` | the parser's `col`, shown as "Ln 3, Col 17" | Not a panic and not a wrong result — a wrong *report*. The caret pointed up to two columns per preceding character too far right. |

The last ten were found while fixing the first seven and were not in the
original count. `gui/clipboard/src/main.rs:183` looked like another but is not:
it already goes through `find_char_boundary`.

The videoplayer one is worth calling out because it does not match the grep
shape above — there is no `if x.len() > N` guard in sight. It is
`format!("{ms_str:0<3}")` followed by `padded[..3]`, and the bug is that
`format!`'s width is counted in **characters** while the slice indexes
**bytes**. For a fractional part of `"ab日"` the padding adds nothing (already
3 characters) and byte 3 lands inside the kanji. So the class is wider than
"a byte budget with a byte guard": it is *any* place where a character count
and a byte count are used interchangeably. Rust's own `format!` width is a
character count, which makes it a natural source of the confusion.

**`apps/renamer` is the one site where the byte/character confusion was also a
*semantic* bug, and the most damaging of the seventeen.** Four rename rules —
insert-at, remove-from, number-at, datestamp-at — slice the filename stem at a
position the *user types into the rule*, clamped only with `.min(stem.len())`,
a byte length. `InsertPosition::At`'s own doc comment has always read "insert at
a specific character index", so the code contradicted its documented intent: for
`日本語.txt`, "insert at 3" is past the end of a 3-character stem and should
append, but the byte clamp put it after the *first* kanji. And unlike a
truncated label, a wrong position here writes the wrong name to disk. The panic
is worse still, because a rename batch applies each rule to each file in turn:
one non-ASCII name aborted the renamer *after* earlier files had already been
renamed, leaving the batch half-applied with no undo record. Fixed with a
`char_offset(s, chars)` helper that all four sites route through, which makes
the position mean what it says and makes the slices sound as a side effect. For
ASCII names the two numbers coincide, so no existing rule changed behaviour —
the pre-existing tests confirm it.

**`apps/markdowneditor` is the largest instance, and the only one where the
bad offset *persists in state* rather than being recomputed each frame.** Every
column in the editor -- `cursor_col`, the selection anchor, the columns recorded
in undo actions -- is a byte offset into a line, which is what lets an edit
apply without re-scanning. Fourteen places kept such an offset in range with
`.min(line.len())`, and a byte length is the wrong bound: it keeps the offset
inside the line but says nothing about whether it lands *on* a character.

Pressing Down is enough to reach it. `move_cursor_down` carries the column to
the next line, so from column 1 of `"abc"` onto `"\u{65e5}x"` the clamp leaves 1,
inside the kanji. Nothing fails yet -- the cursor is simply in an impossible
place. The abort comes on the *next* keystroke, in whichever of Backspace,
Delete, insert, Enter, arrow-key or selection the user happens to press, by
which point the document is unsaved and the user has been typing. Go-to-line, an
undo replayed against a line that changed underneath it, and a reload after the
file changed on disk all reach the same state without any cursor movement at
all.

Fixed with one `clamp_col(line, byte)` that rounds *down* to a character
boundary, used at all fourteen sites. Rounding down puts the cursor at the start
of the character it landed in -- where a user who pressed Down onto a wide
character expects to be -- and for an all-ASCII document it returns exactly what
`.min(line.len())` did, which a test asserts directly.

**`apps/backup`'s `?` wildcard is the only member of the class that never
panics, which is exactly what made it the easiest to miss.** The glob matcher
works on `&[u8]` throughout, which is *correct* — our paths are byte strings
that need not be UTF-8 (`CLAUDE.md` item 7), and rewriting it over `&str` would
have been the wrong fix. But `?` is documented as "any single character except
`/`" and advanced `ti` by one **byte**, so against `日本.txt` it matched one
third of a kanji. `file?.txt` silently stopped matching `file日.txt`. In a
backup tool a pattern that quietly fails to match is worse than a crash: an
exclude that misses copies a directory the user meant to skip, and an include
that misses leaves a file unprotected with the run still reporting success.

Fixed with `utf8_char_len(text, i)`, so `?` advances one character. Only `?`
needed it. `*` is byte-greedy but can only ever *succeed* on a boundary — a
well-formed needle cannot match starting inside another sequence, by UTF-8
self-synchronization — and `/` is ASCII, so it can never occur inside a
multi-byte character.

The interesting part was ill-formed input. The first version clamped a
truncated sequence to the bytes remaining (`want.min(len - i).max(1)`), which
my own test caught as a real defect rather than a wrong expectation: for the
bytes `[0xE6, b'/']` a lead byte announcing three bytes consumes both, and `?`
has crossed a separator — the one thing it must never do. The rule that works
is **validate, then consume**: only treat a lead byte as multi-byte if the
continuation bytes it announces are actually present and in `0x80..=0xBF`,
otherwise advance one byte and let the literal comparison decide. That keeps
the separator invariant and still guarantees forward progress.

**`apps/filesearch` is the same bug as `backup`'s `?`, but as a whole engine
rather than one branch — and it was found by asking "where else does a matcher
step a byte at a time?" rather than by any grep.** filesearch has two engines,
a glob matcher and a small regex matcher, and both stepped `ti` by one byte.
That made *every* single-character construct wrong: `?` and `.`, the character
classes `[...]`, and `\d`/`\w`/`\s` with their negations. It is wrong in both
directions at once, which is what makes it hard to notice from one example:

- **False negatives.** `?.txt` did not match `\u{65e5}.txt`; `h.llo` needed
  three dots for one kanji.
- **False positives.** `\W\W\W` matched exactly one kanji, because every byte
  of a multi-byte character fails `is_ascii_alphanumeric`. `[\u{e9}]*` matched
  `\u{e8}b`, because `\u{e9}` and `\u{e8}` share a lead byte and the class
  compared one byte. A class *range* like `[\u{430}-\u{44f}]` was not merely
  wrong but meaningless — it compared bytes of the endpoints' encodings.

Unlike `backup`, both entry points here take `&str`, so the inputs are already
validated UTF-8 and character semantics is achievable, not just desirable.
Both engines were converted to `&[char]`. That is the whole fix: with `&[char]`
every index is a character index by construction, so `?`/`.`/classes/ranges are
all correct at once and there is no per-construct rule to remember or to get
wrong again later. The public `&str` entry points are unchanged; the two
bulk-search paths gained `*_chars` variants so the pattern is decoded once per
search instead of once per indexed file.

The regression test that earned the most was the *control*:
`an_ascii_pattern_matches_exactly_as_before` pins 20 pre-existing ASCII cases.
Under the deliberate re-break it kept passing while all six non-ASCII tests
failed — which is exactly the evidence wanted, since it shows the six really do
discriminate and that the refactor changed nothing for ASCII input.

Re-breaking this one is worth recording as a technique: rather than reverting
the refactor, the byte engine was reproduced by decoding `.bytes().map(|b| b as
char)` instead of `.chars()`. Every comparison in both engines is by scalar
value, so mapping each byte to the char of the same value restores the old
behaviour exactly, at 8 call sites and with no other edit.

**Asking the behavioural question then found three more sites in two more apps,
which is the strongest evidence that the question is the right tool.** Having
noticed that no grep finds a byte-at-a-time advance, the lane's remaining
matchers, parsers and scanners were read with one question in mind — *does this
walk text one unit at a time, and is that unit a byte?* Three said yes:

- **`dbviewer`'s SQL `LIKE`.** Its own comment reads "`_` matches exactly one
  character"; it consumed one byte. `LIKE '_'` was false for a one-character
  CJK cell while `LIKE '___'` was true for it.
- **`indexer`'s glob matcher** — a third independent copy of the same `?`-and-
  class bug, after `backup` and `filesearch`.
- **`indexer`'s `levenshtein_bounded`.** The most interesting of the three,
  because it is not a wildcard at all: an *edit distance* over bytes charges up
  to 3 for one substituted kanji. Against a `FUZZY_MAX_DISTANCE` the user reads
  as "a couple of typos", a near-exact CJK match was rejected while a much
  worse ASCII one was accepted — and the `abs_diff` length early-out discarded
  candidates before the DP even ran. Fuzzy matching was effectively off for
  non-ASCII names.

That three independent glob matchers in one lane each carried the same defect
is worth noting on its own: this is not a slip someone made once, it is what
you get by default from reaching for `as_bytes()` to walk a pattern. The
generalisation is not "`?` is special" but that **any construct meaning "one
unit of text" is wrong the moment the loop's unit is a byte** — wildcards,
classes, ranges, quantifier counts and edit costs alike.

A second vacuity trap turned up here, of a kind not seen before: **a test can
fail to discriminate because the behaviour that survives the break is genuinely
correct.** `dbviewer`'s first percent-and-literals test passed under the
deliberate break, not through oversight but because `%` and literal matching
really are sound over bytes — the same self-synchronization argument that
cleared `backup`'s `*`. Only a pattern that makes `%` absorb the slack while
`_` must still count (`"日"` against `"%_%_%"`) can tell the two engines apart.
Generalised: when part of a construct is provably safe, a test built from that
part cannot witness the unsafe part, however non-ASCII its input looks.

**The fix was not to hunt for char boundaries at each site.** All but one of
these is a *display* truncation, and each already had a box to draw into, so
each became `guitk::text::elide` / `RenderTree::text_in` (or a `guitk::table`
cell): it measures display width, cuts on a character boundary, and marks the
cut with `…`. That also removed the second, quieter bug present at every site —
a truncation counted in bytes has no relationship to the width of the box the
text is drawn in, so `20` characters of a wide font overflow anyway while `20`
of a narrow one waste half the space.

Two sites needed something other than eliding:

- **`colorpicker::parse_hex_color` is a parser, not a view.** It branched on
  `s.len()` as if it were a digit count. Requiring ASCII hex digits up front
  makes the length a digit count and every offset a character boundary, so the
  rest of the function is sound by construction. (It also closed a smaller
  hole: `u32::from_str_radix` accepts a sign, so `"+FFFFF"` parsed as a colour.)
- **`ClipEntry::text`'s cap is a *retention* bound, not a display one** — a
  clipping can be megabytes and the history holds many. That bound stayed in
  the model but became a character count; the display bound moved to the view.

Three sites had truncation in the *model*, where nothing knows how wide the
drawing surface is: `DragDataType::description`, `NoteStore::sidebar_items`, and
the clipboard row. All three now return full text and the caller elides.

Writing the regression tests turned up four latent layout bugs the byte budgets
had been hiding, all fixed in the same commits: pdfviewer's tab title drew 2px
under its close glyph; flashcards' three columns overlapped below 640px;
procexplorer's memory row sat at a flat 200px pitch and left the panel at 480px
wide; and the clipboard row's meta line could run under the sensitive
indicator.

Grep shape, if this recurs: `&<ident>[..<literal>]` where the receiver is a
`String`/`&str`, and its `if x.len() > N` guard. That shape found seven of the
seventeen; the other ten needed a wider sweep for *any* mixing of the two counts.
Three further forms showed up, none of which the grep can see: `format!` width
(a *character* count) meeting a byte slice (videoplayer); `.min(s.len())` used
to clamp a position the user thinks of in characters (renamer,
markdowneditor); and a byte-at-a-time advance where a character was meant
(backup's `?`, both of filesearch's engines), which involves no slicing and no
`len()` at all.

That last form is the one to go looking for next, because no textual pattern
finds it — it is `ti += 1` in a loop, which matches everything. The question
that finds it is behavioural: **"does this walk text one unit at a time, and is
that unit a byte?"** Both remaining instances were found by asking it of every
matcher/parser/scanner in the lane rather than by grepping. Note this is the same root
cause as the unbounded-column survey below — **counting characters instead of
measuring the box** — and it was worth treating as one problem.

Every fix is covered by a test using Japanese/Greek/Russian/emoji input plus a
string pinning the exact cut index to a continuation byte, and every one was
verified non-vacuous by re-breaking the production code and confirming the test
fails. That discipline earned its keep five times here:

- `colorpicker`'s `chars[2]` index was in fact *unreachable* -- `hex_char_to_u8`
  rejects a multi-byte char one step earlier -- so the "second panic" claimed
  for that site did not exist.
- An earlier `file_drop` test passed with its bound removed, because no
  reachable payload draws both a count badge and a long description.
- `markdowneditor`'s first sweep drove each edit through `move_cursor_down`, so
  breaking any *edit* site changed nothing: the sweep already aborted on the
  cursor-position assertion from the vertical move, one case earlier. Five
  sites looked verified and were not. Replaced with a test that strands the
  column directly, which both isolates each site and matches reality, since
  undo replay and click-positioning strand it without any vertical move.
- `markdowneditor`'s reload clamp passed with the clamp removed -- no test
  reached it -- until a test was added for it specifically.
- `backup`'s "`?` never crosses a separator" test passed under the very break
  it existed to catch: `?c` against `[0xE6, b'/', b'c']` fails for an unrelated
  reason, so the assertion never distinguished the two versions. Pinned with
  `assert!(!glob_match_recursive(b"?", &[0xE6, b'/']))`, which does. A test
  aimed at an invariant is not the same as a test that can *see* the invariant
  break.

General rule this keeps re-teaching: **when several defects can abort, break
them one at a time**, and be suspicious of a break that leaves the failure
count unchanged -- it usually means the new failure is the old one.

One further trap, from this same session: do not re-break production code while
a full-workspace test run is in flight. A workspace gate launched earlier picked
up `renamer` mid-verification and reported two failures that were the
scaffolding, not the tree.

**Site eighteen shows the class reaches things that neither panic nor compute a
wrong answer.** `apps/jsonviewer`'s parser counted `col` once per byte. Nothing
downstream indexes with it — it is used only to *tell the user where the error
is*, in the status bar and the error list. So the parse was right, the error was
right, and the caret pointed at the wrong character: a document whose string
value is `日本語` rather than `xxx` reported column 20 where the ASCII one
reported 14. That makes it the least dangerous instance and the easiest to
overlook, because there is no crash and no bad data to notice — just a number
that quietly stops meaning what its label says. The fix is one line: skip the
increment for continuation bytes (`b & 0xC0 == 0x80`), which are the tail of a
character its leading byte already counted.

**A caution about how these are found.** The same grep that turned up kanban's
real corruption (next section) also flagged `apps/jsonviewer`'s `parse_string`,
which does `result.push(b as char)` on the very next line — and *that* one is
correct, because it sits under `if b < 0x80` and the non-ASCII branch rewinds
into a real UTF-8 decoder with proper surrogate handling. Two functions, the
same six-token expression, opposite verdicts. No pattern distinguishes them;
only reading the enclosing guard does. Treat a grep hit in this class as a
question, never as a finding.

Violates `CLAUDE.md` self-review item 7 (never force UTF-8 assumptions on
OS-boundary data) and trips the workspace's `clippy::indexing_slicing` warn.


## `u8 as char` reinterpreted UTF-8 as Latin-1 in four parsers (lane C)

**Status: FIXED 2026-08-15** (lane C, commits `237636350` kanban, `3b6b60e39`
backup, `18f1e9abc` rssreader). Found while sweeping for byte-at-a-time text
walkers, and it is a *different* class from the byte/character-count confusion
above — worth keeping separate, because the symptom, the detection method and
the fix all differ. Three JSON readers and one XML reader carried it.

`apps/kanban`'s `JsonImporter::parse_string` built its result one byte at a
time:

```rust
} else {
    result.push(b as char);   // b: u8
}
```

`b as char` maps a **byte value** to the Unicode scalar with that value. That is
a Latin-1 decode. There is no count involved, nothing is truncated, and nothing
panics: an imported card titled `日本語` (E6 97 A5 ...) simply comes back as
`æ\u{97}¥...`. It is `String::from_utf8_lossy`'s failure mode reached by a
different route, and it is worse than a panic in one specific way — **the damage
persists.** The mojibake becomes the card's title in memory, and the very next
save writes it to disk as the new truth. Import a board, glance away, and the
original text is gone.

Why the count-confusion sweep would not have found it: there is no `len()`, no
slice, no guard, no wildcard. The tell is the cast itself. The generalisation
worth carrying forward is that **`u8 as char` is almost always a bug on text**;
it is sound only where the byte is already known to be ASCII — which is exactly
the distinction that made `apps/jsonviewer`'s identical-looking line correct.

The fix copies unescaped runs out as whole `&str` slices instead. That is sound
precisely because the two bytes that terminate a run — `"` and `\` — are ASCII,
and an ASCII byte can never occur inside a multi-byte UTF-8 sequence, so the cut
is always on a character boundary. (This is the same self-synchronisation
property that cleared so many near-misses in the sweep above; here it is what
makes the fix work rather than what made the bug absent.) It is also faster than
pushing char by char.

Fixing the function properly turned up two further defects in it:

- **`\uXXXX` was never decoded.** It fell through to the unknown-escape arm and
  came back as a literal backslash, `u`, and four digits. This was not
  hypothetical: our own `JsonExporter::escape_json` emits exactly that form for
  every character below U+0020, so **export followed by import did not
  round-trip** for any card whose text contained a control character. Now
  decoded, including leading/trailing surrogate pairs — which is how JSON spells
  anything outside the BMP, so emoji in a board exported by any other tool were
  equally unreadable. An unpaired surrogate degrades to U+FFFD rather than
  failing the whole import.
- **The unknown-escape arm had the same cast** (`result.push(esc as char)`) on a
  single byte, so a backslash followed by a multi-byte character both corrupted
  that character and left the scan stranded mid-sequence. It now consumes a
  whole character.

Non-vacuity was checked by reinstating the byte-at-a-time parser: all four new
tests fail under it while the ASCII control test and both pre-existing parser
tests keep passing — the profile that shows the new tests discriminate *and*
that the rewrite changed nothing for ASCII.

Violates `CLAUDE.md` self-review item 7 in its strongest form: this is not an
assumption about encoding, it is an actual re-encoding.

### The same bug in `apps/backup`'s manifest reader — the worst instance

`apps/backup`'s `parse_string` had the identical cast, but on data that makes it
far more consequential: **the strings in a backup manifest are file paths.** A
backup of `写真/2024.jpg` reads back as a path naming no file at all, so restore
cannot find it and verify reports it missing. The manifest is the only record of
what was backed up; corrupting it silently invalidates the archive.

Fixing it turned up two further defects in the same function, both worse than
the first:

- **A reachable panic.** The `\u` arm did `&input[i + 1..i + 5]` with no bounds
  or boundary check. On `"\u日本"` that cuts at byte 7, inside `本`, and Rust
  panics on the non-boundary slice. Reachable from merely *reading a manifest
  off disk* — no attacker needed, just a path that happens to follow a
  backslash-u with non-ASCII text. `parse_hex4` now uses
  `input.get(start..end).ok_or("incomplete unicode escape")?`.
- **Silent data loss on astral characters.** The `\u` decode used a bare `u16`
  with no surrogate pairing and `if let Some(c)` with *no `else`* — so an
  escaped emoji or CJK-extension character in a path did not become U+FFFD, it
  simply **vanished**, shortening the path to something else entirely. Now
  paired properly, with U+FFFD for an unpaired surrogate.

Three defects, so non-vacuity was checked with three separate breaks, each
confirmed to fail only its own tests while the ASCII control kept passing. The
last test builds a real `Manifest` of non-ASCII `FileEntry` paths and
round-trips it through `serialize`/`deserialize`. 46 tests pass.

### The same bug in `apps/rssreader` — the only remotely-fed instance

`XmlParser::read_attribute_value` accumulated with `value.push(b as char)` on
bytes straight off a downloaded feed, so any non-ASCII enclosure URL, title or
author arrived as mojibake. What makes this one instructive is that it was the
**outlier in its own file**: `read_until`, `read_name` and the text-node reader
all already sliced the range out whole and used `from_utf8_lossy` — which is
exact here, since `parse_xml` takes a `&str` and every delimiter is ASCII. The
correct pattern was sitting three functions away. Fixed to slice the same way.

Fixing it exposed an unrelated robustness defect in the same path, arguably more
damaging in practice than the mojibake: `decode_entity` returned `Err` for
anything outside XML's five entities, and `read_attribute_value` propagated it
with `?`. So **one `&nbsp;` in any attribute failed the parse and threw away the
entire feed** — as did a bare `&` in a query string, which is ubiquitous in
enclosure URLs. The same entity in a *text node* merely rendered literally,
because that caller fell back to the raw string; only the attribute path was
fatal. `decode_entities` is now infallible: unrecognised entities are emitted
exactly as written, bare `&` passes through, and twenty-six common HTML entities
are decoded rather than left as source text. Two breaks, two disjoint failure
sets. 147 tests pass.

The pattern across all four: **the cast is never the only bug in the function.**
Every site that had it also had at least one other defect in the same escape or
delimiter handling — a panic, a dropped character, or a fatal error on ordinary
input. Byte-at-a-time text handling seems to correlate with not having thought
about the hard cases at all.


## The file explorer's paste and delete were a weaker duplicate of its own engine (lane C)

**Status: FIXED 2026-08-15** (lane C, commit `bcd1e2d5d`). Found while auditing
`to_string_lossy` uses in `apps/explorer` — those turned out to be fine (the
real `PathBuf` is always kept as the truth and the lossy `String` is only ever
displayed), but the surrounding code was not.

`apps/explorer/src/fileops.rs` is a complete file-operation engine: plans with a
conflict policy, a crash-recovery journal, per-file error policy, progress, an
undo stack, and a `RecycleBin` that stores each item with its original path so
it can be listed and restored. `apps/explorer/src/main.rs` used **none of it.**
`paste()` and `delete_selected()` called `fs::copy` / `fs::rename` /
`fs::remove_file` in a loop, discarding every `Result` with `let _ =`. Three
distinct silent failures followed:

- **Paste destroyed an existing file of the same name.** `fs::copy` overwrites
  its destination, so pasting `notes.txt` into a folder that already had one
  replaced it with no prompt, no rename, no undo.
- **"Move to recycle bin" produced files that could not be restored — and
  destroyed each other.** It renamed into a flat `/var/recycle` with no
  metadata, so `RecycleBin::list` never saw the item and `restore` had no
  original path to restore *to*. And because `fs::rename` overwrites, deleting a
  second `notes.txt` from a different directory silently destroyed the first
  one already sitting in the bin. The recycle bin was, in effect, a shredder
  with an unreliable name-collision hazard.
- **The status line reported unconditional success.** "Paste complete" whether
  or not any file copied; "N item(s) deleted" where N was the number
  *selected*, not the number that worked.

The fix routes both through the existing engine rather than patching the
duplicate — `copy_dir_recursive` is deleted, not repaired. Two implementations
of one operation is precisely how the weaker one ends up on the user-facing
path; `CLAUDE.md`'s "watch for band-aid accumulation" rule names this shape.

**A fourth defect surfaced only when the tests were written.** Every operation
ends by calling `load_directory()`, which calls `update_status()`, which
overwrote `status_message` with the folder/file summary. So no operation result
was *ever* visible to the user — not a paste, not a delete, not a rename, and
not the `Error: {e}` set when `read_dir` fails, which was assigned and then
discarded two lines later (an unreadable directory rendered as "0 folder(s), 0
file(s) — 0 B"). The transient result and the derived summary are now separate
fields, with `status_bar_text()` preferring the result and navigation clearing
it.

**The root cause behind all four: `apps/explorer/src/main.rs` had no
`#[cfg(test)]` module at all.** `columns.rs`, `dropzone.rs`, `fileops.rs` and
`thumbs.rs` are all well tested; the file holding `ExplorerState` — navigation,
selection, clipboard, paste, delete, rename — had zero tests. It now has 11,
with each of the three behavioural fixes verified non-vacuous by a separate
break. Worth generalising: **a well-tested support module is not evidence that
the code calling it is tested**, and in this crate the untested file was the one
users actually touch.

Two smaller defects in `fileops.rs` noticed during the same read — **also FIXED
2026-08-15**, commit `35f17dfd7`:

- `RecycleBin::recycle` moved data with a bare `fs::rename`, which fails with
  `EXDEV` across a mount point — so deleting anything from a separate data
  partition simply errored, and `restore` had the same problem in reverse. Now
  routed through a `move_path` helper that tries the rename and falls back to
  copy-then-remove. Note that `fileops::same_device` exists for exactly this
  check and was referenced only by its own test (`dropzone.rs` carries a second
  copy of the same function); it was not used here, because attempting the
  rename and reacting to its failure is both cheaper in the common case and
  correct where a first-component heuristic guesses wrong.
- `RecycleBin::recycle` wrote the original path to `meta.txt` with
  `path.display()`, which is lossy, and `read_entry` parsed it back as UTF-8 —
  so a non-UTF-8 path was restored under a *different name*, silently renaming
  the user's data during an operation advertised as reversible. Same class as
  the `u8 as char` section above, reached through `Display` instead. The path is
  now percent-encoded from `OsStr::as_encoded_bytes`, with a version marker on
  line 1 so an already-populated bin is still readable.

A third, unlogged defect fell out of writing those tests: metadata was written
before the data was moved but not removed if the move failed, so a failed
recycle left the bin listing an entry whose `data/` was not there. Ordering
kept (metadata first is the safe order — orphaned metadata is harmless, moved
data with no metadata is unrestorable), with cleanup on the failure path.


## Fixing a parser is not fixing a format: `apps/backup` corrupted paths on the *write* side (lane C)

**Status: FIXED 2026-08-15** (lane C), commit below. This is a follow-up to the
`u8 as char` section above, and the lesson is the one worth keeping.

Commit `3b6b60e39` fixed `apps/backup`'s manifest **reader** — the JSON string
parser that re-encoded UTF-8 as Latin-1. That looked like the whole bug. It was
not. `FileEntry.path` was a `String`, and every one of those strings was
produced by `relative_path`, which did
`full.to_string_lossy().replace('\\', "/")`. So a filename the filesystem
happily stored — our paths allow every byte but `/` and NUL — was flattened to
U+FFFD *before the manifest writer ever saw it*. A backup of `café.txt`
(0xE9, not UTF-8) recorded `caf<FFFD>.txt`; restore recreated the file under a
different name, and `verify` reported the original as missing. The archive was
self-consistently wrong, so nothing downstream could detect it.

**Generalization: when you find a lossy conversion in a parser, the format has
two sides — go find the writer.** A round-trip test through the parser alone
passes vacuously, because the corruption happened upstream of the data the test
constructs by hand. The reader fix and its tests were both real and both blind
to this.

Fixed by making the path a `PathBuf` end to end: `relative_path` now strips the
base and rejoins components on `/` at the byte level, and the manifest stores
paths percent-encoded from `OsStr::as_encoded_bytes` — the same escape and
version-marker scheme just adopted for the recycle bin's `meta.txt`. A manifest
with no `version` field is read as version 1 (paths verbatim), so archives taken
before this change still restore.

Three further defects fell out of the work:

- `detect_changes` contained a dead push/pop pair and two empty `if` bodies
  left over from a half-finished edit; it computed `modified` twice, and the
  first computation was discarded. Rewritten as a single pass. Behaviour is
  unchanged — the hash comparison was always the one that counted — but the
  size/mtime "quick check" it pretended to do was never wired to anything.
- The file-type breakdown in `stats` used `path.rsplit('.').next()`, which
  reports the whole filename as the extension for `README` and `.gitignore`
  alike. Now `Path::extension`.
- Both new percent-decoders (here and in `fileops.rs`) built their `OsStr` with
  `OsStr::from_encoded_bytes_unchecked` under a SAFETY comment claiming every
  byte string is valid for the platform's encoding. That is true on Unix and
  **false on Windows**, where `OsStr` is WTF-8 — and Windows is the host the
  tests run on, so the unsound branch was the only one ever executed. Replaced
  with a `#[cfg(unix)]` split: `OsString::from_vec` (safe and total) on the
  real target, which is `target-family = ["unix"]`, and a documented
  best-effort on the test host. The lossless core is now byte-level
  (`encode_bytes`/`decode_bytes`) and tested there, so the round-trip is
  asserted at the level the file is actually written at rather than at a level
  the test host cannot represent.

Related tooling gap, now closed: `rustup target add x86_64-unknown-linux-gnu`
was not installed, so **no `#[cfg(unix)]` code in this lane had ever been
compiled**, let alone checked. `cargo check --target x86_64-unknown-linux-gnu`
needs no linker and now covers those branches.


## `apps/indexer` stored index paths lossily and panicked on a short header (lane C)

**Status: FIXED 2026-08-15** (lane C), commit below. Third instance of the
lossy-path class, found by continuing the sweep. The index is a binary,
length-prefixed format, so unlike `meta.txt` and the backup manifest there was
never a readability tradeoff to weigh — it simply stored the wrong bytes:

- `serialize` wrote `entry.path.to_string_lossy().as_bytes()` and
  `deserialize` read them back with `String::from_utf8_lossy`. A file whose
  name is not UTF-8 was indexed under a name containing U+FFFD, so the search
  hit that named it could not be opened. Both sides now carry
  `OsStr::as_encoded_bytes` verbatim; `INDEX_VERSION` goes 1 → 2. No migration
  is needed — the index is a derived cache and the existing version check
  already tells the user to reindex.
- **Panic on a truncated index.** The header check was `data.len() < 28`, but
  `dirs_scanned` is read from bytes `24..32`, so a file of 28..=31 bytes
  passed the check and then indexed out of bounds. The existing
  `test_index_deserialize_too_short` used a 4-byte input and never reached it.
  Now `< INDEX_HEADER_LEN` (32), with a test that sweeps every length below it.

Two smaller things fixed in passing: the two scanners each carried a verbatim
copy of the directory-exclusion check (now one `is_excluded_dir`), and each
copy tested `dir_str.ends_with(excl) || dir_str.contains(excl)` — the same
predicate written twice, since `contains` is true whenever `ends_with` is.

The `filename` field stays a lossy `String`, now documented as a **search key
only**: a query is UTF-8 text the user typed, so matching against a lossy
rendering is a selection heuristic. It is never displayed and never used to
name a file — `path` is, and `path` is exact. Both producers of the key now go
through one `filename_key` function so they cannot drift.


## The thumbnail cache keyed on a lossy path, so one file showed another's image (lane C)

**Status: FIXED 2026-08-15** (lane C), commit below. Fourth instance of the
lossy-path class, and the first where the damage is not a lost name but a
**collision**.

`Thumbnail::source_path` was a `String` built with `to_string_lossy`, and it is
the disk cache's key: `simple_hash` FNV-hashes it with the mtime to produce the
cache filename. Every undecodable byte in a name became the same U+FFFD, so two
genuinely different files whose names differ only in such bytes hashed to one
cache entry — and the file explorer displayed one of them the other file's
thumbnail. Nothing errors; the wrong picture is simply shown. `source_path` is
now a `PathBuf` and `simple_hash` takes `&Path` and hashes
`as_os_str().as_encoded_bytes()`.

`purge_stale` had a matching problem in the other direction: it compared
directory entries by `to_string_lossy`, so a foreign file in the cache
directory whose name is not UTF-8 could be *rendered into* something matching
our `{hash:016x}.thumb` shape and deleted. Now compared as bytes.

**The lesson worth keeping is about the tests, not the bug.** The natural
regression test for this class needs a path the platform cannot decode, and on
the Windows test host `OsString` cannot hold arbitrary bytes at all — so the
obvious test has to be `#[cfg(unix)]` and never actually *runs* here. A
`cfg(unix)` test is compile-checked at best (and until this session, not even
that — see the note in the `apps/backup` entry above). The fix is to find the
host's *own* uncodable case: on Windows an unpaired surrogate is a legal
`OsString` that `to_string_lossy` maps to U+FFFD, which reproduces exactly the
same collision. Both tests now exist, and the Windows one was confirmed to fail
when the lossy hash is put back. Any future test in this class should carry a
runnable-on-the-host twin rather than a Unix-only assertion.


## TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED — `max_width` cuts mid-glyph and says nothing — ✅ **RESOLVED 2026-08-15**

**Resolution.** `RenderCommand::Text` gained a **required** `overflow:
TextOverflow` field (`Clip` | `Ellipsis`), and the compositor draws the mark.
The operator chose "required, no `Default`" from four options precisely so that
every one of the 4,517 constructions in the tree had to answer the question
`max_width` had been posing and never answering; see `design-decisions.md` §427
for the options and §429 for why the commit also had to fill in lane B's 31
sites. The second measurement the entry complains about below is gone from the
policy path: the compositor decides about the mark from the run it has already
shaped, so `text::elide` is no longer the only way to get a cut marked.

Bounded sites default to `Ellipsis` rather than to the behaviour-preserving
`Clip`, because today's behaviour *is* this entry — a sweep that faithfully
preserved it at four thousand sites would have done nothing.

Tested at all three layers: the compositor (a mark appears only when earned,
stays inside the limit, falls back to clipping when the mark itself does not
fit, and never blanks a field clipping would have filled), the toolkit (each
helper emits the right policy), and `guiremote` (both policies survive the wire,
are distinguishable on it, and an unknown byte is a `DecodeError` rather than a
guess — `PROTOCOL_VERSION` went to 2 for it).

**Status.** ~~Open, and deliberately not fixed in the pass that closed
`TD-GUI-TEXT-COMMAND-DOES-NOT-WRAP`, because the good fix is a change to
`RenderCommand::Text` itself and wants a decision rather than a sweep.~~
The decision was asked and answered.

**What it is.** `max_width` clips: the compositor walks glyphs and stops when
the next one would cross the limit. It draws no ellipsis. So a label that does
not fit ends mid-word — and, worse, ends *plausibly*: "Gateway 192.168.1.1 res"
reads as a complete string to anyone who cannot see the field it was cut from.
A caller that wants the cut marked has to call `text::elide` first, which
measures the string a second time to answer a question the compositor is about
to answer again while drawing. That is the same two-calculations-for-one-quantity
shape as the wrap bug, one layer down.

**How widespread.** Every single-line label in the app tree that passes
`max_width: Some(..)` without eliding first — well over a hundred sites. Most
are fine in practice because the values are short and app-authored; the ones
that bite are those carrying user or network data (file names, SSIDs, error
strings, host names). `netmanager`'s diagnostics detail line was fixed by hand;
the rest were left.

**Proper fix.** Give the command an explicit overflow policy — `Clip` (today's
behaviour, correct for a progress label that must not jitter) versus `Ellipsis`
(the right default for a data-bearing label) — and let the compositor draw the
mark, since it is the only party that knows exactly where the glyphs ran out.

**Why it is not done.** Adding a field to `RenderCommand::Text` touches every
struct-literal construction of it in `gui/**` and `apps/**` — several hundred —
because Rust has no default for a struct-variant field. The alternatives are
each a compromise: a second variant (`TextClipped`) splits the match arms in
every renderer; a builder function leaves the literal form available and so does
not actually prevent the mistake; a blanket `text::elide` sweep at the call
sites fixes the symptom while keeping the double measurement. The mechanical
churn is cheap to *do* and expensive to *review* against three lanes' in-flight
work, so it should be scheduled deliberately rather than smuggled into an
unrelated fix. Recorded for the operator in `open-questions.md`.

---

### TD-FONT-NOT-ACTUALLY-NO-STD. `osfont` documents itself as `no_std` but links `std` — 2026-08-14 — OPEN

**What.** `gui/font` is written entirely in `alloc` terms (`alloc::vec::Vec`,
`alloc::string::String`, no `std::` paths, `extern crate alloc;` at the top),
and a comment in `cff.rs` asserted outright that "this crate is `no_std`". It
is not: `src/lib.rs` carries no `#![no_std]` attribute, so the crate links the
standard library like any other and the discipline is enforced by nothing but
habit.

**How it was found.** Adding `#![no_std]` to see whether the claim held. It
does not — the build fails with 47 errors, in two groups:

- **Float math (35 errors).** `f32::sqrt`, `floor`, `ceil`, `round` and
  `mul_add` are inherent methods provided by `std`, not by `core`. They are
  used throughout `raster.rs` and `scaled.rs`, which is unavoidable for a
  rasterizer.
- **Prelude items (12 errors).** `String`, `vec!` and `format!` are reached
  through the `std` prelude at a dozen sites instead of being imported from
  `alloc`.

**Why it matters.** The compositor and the toolkit both depend on this crate
and both are meant to run on SlateOS. As long as the attribute is absent, a
`std::`-only construct added here compiles cleanly on the development host and
fails only when someone finally builds for the target — at which point the
offending code is old and its author is a previous session. The false comment
made this worse than a silent omission, because it told the next reader the
invariant was already being checked.

**Proper fix.** Add `libm` to the workspace, replace the inherent float
methods with `libm::{sqrtf, floorf, ceilf, roundf, fmaf}` (or the
`num-traits`/`libm` float shim), import the prelude items from `alloc` at the
dozen sites, then add `#![no_std]` and `#[cfg(test)] extern crate std;`. The
mechanical part is small; what makes it more than mechanical is that `libm`
would be this workspace's first float-math dependency, and whether SlateOS
userspace GUI binaries get a `std` port at all is Lane B's call (`posix/**`) —
if they do, `no_std` here buys much less than it seems to. That question
should be settled before spending the churn.

**Interim.** The false comment in `cff.rs` was corrected and the crate docs in
`lib.rs` now state the real position, so nobody is misled into thinking the
invariant is enforced. Keep writing `alloc::` paths: the point of doing so is
that closing this stays a small change.

---


## FIXED: TD-START-MENU-POWER-ROW-IS-A-LABEL

**Fixed 2026-08-14.** The footer row is a real button: `power_button_rect()`
reports `Hit::PowerButton`, which toggles `power_menu_open`, and
`power_menu_rect()` / `power_menu_row_rect(row)` place a popup that
`power::render_power_menu` draws and `hit_test` reads — one accessor per
clickable part, as the `Rect` documentation requires. Its five rows are
`power_menu_entries()`, exactly the `Category::System` entries that
`start_menu_entries()` filters out, and clicking one returns the same
`ShellAction::Launch` an application row does: `/sbin/shutdown` and its
neighbours are what actually shut the machine down, not the window manager.
The popup is themed and scaled by the shell (it takes a `PowerMenuStyle`)
rather than by `power.rs`'s own palette, so it follows the light theme and the
display scaling like everything else. `close_start_menu()` is now the single
place the menu closes, which is what keeps the submenu from being stranded over
an empty desktop. Nine tests in `pointer_tests.rs`, including one that walks
every scale from 100% to 200% asserting no system action is dropped or drawn
where it cannot be clicked. No confirmation prompt: Start → Power → Shut down
is one click on every desktop that has this menu, and an extra "are you sure"
is not what makes shutdown safe.

The original report follows.

**What.** The foot of the start menu draws the word "Power" in grey. It is
text, not a control: `hit_test` reports `Hit::StartMenuPanel` there, and the
five `Category::System` entries of the app database — Shutdown, Restart,
Sleep, Lock, Logout — are consequently unreachable from the shell. They are
filtered *out* of `start_menu_entries` on purpose, so that "Shutdown" is not
one mis-click below "Screenshot"; but nothing yet offers them anywhere else.

**Why it bites.** There is no way to shut the machine down from the desktop.

**Proper fix.** A power submenu opened from that row: a small popup listing the
`Category::System` entries, which resolves to the same `ShellAction::Launch`
the application rows produce. `gui/desktop/src/power.rs` already models power
actions and confirmation prompts and should be the thing that renders it,
rather than a second list inside `render_start_menu`. Needs the same
geometry-shared-with-the-hit-test treatment as the rows above it — see the
`Rect` documentation in `main.rs`.

**Where.** `gui/desktop/src/main.rs` — `render_start_menu`'s footer,
`DesktopShell::hit_test`; `gui/desktop/src/power.rs`.


## FIXED: FLAKY-GUITK-SCALING-TESTS-SHARED-A-PROCESS-GLOBAL

**What.** Five tests in `gui/toolkit/src/scaling.rs` — `global_scale_default_is_1`,
`set_and_get_global_scale`, `global_scale_clamped`, `per_monitor_override`,
`per_monitor_clear_falls_back` — each wrote the process-wide `SCALE_TABLE` and
then asserted on it. Cargo runs tests on parallel threads, so
`per_monitor_clear_falls_back` (which sets the global scale to 1.5) failed
whenever another of the five reset it to 1.0 in between. Observed failing once
in a full `cargo test -p guitk` run and passing when run alone.

**Fix.** A `SCALE_LOCK` mutex in the test module, taken by a `ScaleGuard` whose
`Drop` restores the whole table. Restoring on drop rather than at the end of
each test body means a failing assertion — which unwinds — still leaves clean
state, so one failure cannot cascade. The lock is taken with
`unwrap_or_else(|e| e.into_inner())` because a poisoned lock carries no
information once the guard restores the state anyway.

**Residual gap.** `DesktopShell::set_appearance` now publishes the display
scaling into that same process-global table, so `guitk` widgets hosted in the
shell lay out at the scale the chrome is drawn at. That one line has no unit
test of its own: every desktop test that builds a shell writes the value an
assertion would read, and `desktop` is a binary crate so the assertion cannot
be moved to an out-of-process integration test. Rationale is recorded on the
method.

**Where.** `gui/toolkit/src/scaling.rs` (test module);
`gui/desktop/src/main.rs` — `DesktopShell::set_appearance`.


## TD-GSUB-APPLIES-EVERY-SCRIPTS-FEATURES — ✅ FIXED 2026-08-14

**What.** The `GSUB`/`GPOS` walk in `gui/font/src/otl.rs` starts at the
FeatureList and takes *every* feature carrying a wanted tag, rather than
starting at the ScriptList and taking the features that the run's script and
language actually select. A face that registers the same feature tag under
several scripts therefore has all of those scripts' lookups applied to every
run, whatever the run is written in.

**Why it bites now.** This was a documented, mostly-theoretical limitation
while only `liga`/`rlig` were read: a ligature belonging to another script
almost never matches Latin glyphs, so the wrong lookups ran but did nothing.
Reading `ccmp` changes that. `ccmp` is precisely where a script puts its
normalisation rules, and those rules are meaningless — or wrong — outside it.

**Reproduce.** `cargo test -p osfont --target x86_64-pc-windows-gnu --test
host_fonts -- --ignored --nocapture installed_fonts_leave_plain_latin_alone`.
On a stock Windows host, `ebrima.ttf` and `ebrimabd.ttf` substitute the *space*
glyph in plain English prose: their `ccmp` lookup 15 is an extension-wrapped
type-1 format-2 subtable mapping glyph 3 (space) to 2220, and it belongs to one
of the African scripts Ebrima covers, not to Latin. Verified against an
independent Python parse of the table, so this is our *selection* being wrong,
not our *parsing*.

The damage is small — 2 faces of the 275 with `GSUB` on this host, and the
substituted glyph is a space variant — but it is a genuinely wrong glyph, and
the class of fault grows with every feature added.

**Proper fix.** Script and language selection, in two parts:

1. **The table walk.** Walk the ScriptList, pick the ScriptRecord for the run's
   script (falling back to `DFLT`), then its LangSys (falling back to the
   default), and intersect that LangSys's feature indices with the wanted tags.
   This is contained work in `otl.rs` and affects `kern.rs` and `mark.rs` too,
   since they share the walk.
2. **Script itemisation.** Deciding what a run's script *is* needs the Unicode
   Script property, which this crate does not have — a run must be split into
   same-script pieces before it can be shaped, which is also the prerequisite
   for bidi and for complex-script reordering. This is the larger half and is
   the reason (1) is not enough on its own.

Until both land, `installed_fonts_leave_plain_latin_alone` tolerates a small
proportion of faces changing plain Latin prose. When script selection works,
that count should drop from eight to the six Linux Libertine files, whose `Th`
ligature is correct.

**Where.** `gui/font/src/otl.rs` — `lookup_indices` (the FeatureList walk, and
the module doc's "What is not here"); `gui/font/src/gsub.rs` — the feature tag
list in `Substitutions::parse`; `gui/font/tests/host_fonts.rs` —
`installed_fonts_leave_plain_latin_alone`.

**Fixed 2026-08-14** (commit `6e0746636`), both parts, as designed above and
recorded in design-decisions.md §411.

1. `ByScript` in `otl.rs` walks the ScriptList, resolves every script the face
   registers once at parse time, and shares the decoded lookups keyed by
   LookupList index. `Substitutions::apply` takes the run's script and binary
   searches for it, falling back `dev2`→`deva`→`DFLT`→`dflt`.
2. `gui/font/src/script.rs` carries the Unicode Script property (generated
   into `script_tables.rs` from `fontTools.unicodedata`) and `script::runs`
   splits a piece list into maximal same-script stretches. The split happens
   in `ScaledFont::shape` *before* substitution, while glyphs are still one
   per piece — after anything ligates, a boundary counted in pieces is no
   longer a boundary counted in glyphs.

Ebrima no longer substitutes the space, and the same change fixed
`B-FONT-CALIBRI-SHAPES-A-FRACTION-SLASH-DIFFERENTLY-FROM-HARFBUZZ`, whose
cause turned out to be identical.

**The prediction in this entry was wrong, and the correction is the
interesting part.** It said the plain-Latin count "should drop from eight to
the six Linux Libertine files". It is *nine*, and all three non-Libertine
faces are correct: `segoesc`/`segoescb` have genuine Latin `calt`, and
`SansSerifCollection` maps `space` through its Latin `locl` — a feature this
crate had been skipping, and which was only safe to add once features were
script-scoped. All nine now agree with HarfBuzz glyph for glyph. The test's
bound is a proportion, not a list, which is why it kept working; a hard-coded
expected count would have had to be relaxed for a change that made the shaper
*more* correct.

**Successors.** Four narrower gaps remain and are filed separately:
`TD-FONT-IGNORES-LANGSYS-OVERRIDES`,
`TD-GPOS-APPLIES-EVERY-SCRIPTS-FEATURES`,
`TD-FONT-SCRIPT-RUNS-IGNORE-SCRIPT-EXTENSIONS` and
`TD-FONT-HAS-NO-JOINING-OR-REORDERING-SHAPER`.


## TD-FONT-IGNORES-LANGSYS-OVERRIDES — a font's per-language rules were unreachable — ✅ **RESOLVED 2026-08-15**

**Resolution.** Exactly the fix sketched below, and the required-feature gap
beside it. `ScaledFont::shape_lang(text, Option<Lang>)` and
`SystemFont::shape_lang` take a language; `shape(text)` is `shape_lang(text,
None)`, so the change is purely additive and no caller that names no language
can shape differently than it did. `otl::ByScript::parse` now precomputes a
lookup selection per **(script, language)** rather than per script, preferring
the named LangSysRecord over the DefaultLangSys, and `feature_indices` finally
reads `requiredFeatureIndex` — the one feature a language system states outside
its index list, which the walk had been dropping for the default language too.

`lang.rs` does the BCP 47 → OpenType mapping, following HarfBuzz's
`hb_ot_tags_from_language` rule for rule: complex rules first (`ro-MD` →
`MOL `, `zh-Hant` → `ZHT `), then extended-language-subtag substitution, then
the 2- and 3-letter registries, then the blocked list, else uppercase. It is
allocation-free and puts no bound on the tag's length.
`tools/gen_lang_tables.py` generates its four tables from HarfBuzz's source, so
a registry update is a regeneration rather than an edit.

Four things worth keeping in mind about the shape of the fix:

- **A LangSysRecord replaces the default's feature list; it does not add to
  it.** So naming a language can take a feature *away* — which is exactly what
  `TRK ` does to `liga`. Callers should pass `None` rather than a guess: a
  wrong language is worse than no language.
- **One BCP 47 tag resolves to a *list* of up to three OpenType tags, not to
  one.** `ro-MD` is `MOL ` and then `ROM `; `ml` is Malayalam Traditional and
  then Reformed; `ga` is `IRI ` and then `IRT `. They are candidates and not
  synonyms: a face is asked for each in turn and the first it **registers**
  wins. The cap of three is HarfBuzz's `HB_OT_MAX_TAGS_PER_LANGUAGE`, and
  truncating where HarfBuzz truncates is what keeps the two engines answering
  alike. See "What the oracle caught" below — the first version of this fix
  kept only the head of each list and was wrong on 66 of the host's 556 faces.
- **Language selection deliberately does not fall back the way script
  selection does.** A script that does not register the language takes its own
  default, never another script's — HarfBuzz's split between
  `hb_ot_layout_table_select_script` and
  `hb_ot_layout_script_select_language`. `gsub::tests::language_selection_does_not_fall_back_to_another_script`
  pins it.
- **A script's default entry is stored even when it selects nothing**, because
  that entry is what says the script exists and stops the fallback chain.
  Language entries identical to their script's default are *not* stored; two
  thirds of the host's are. This is why `ByScript` keeps a second list of every
  (script, language) the face *registers*: "which candidate wins" is decided by
  what the font registered, never by what happened to be worth storing, or a
  `MOL ` that says nothing would hand Moldavian to `ROM `'s overrides on the
  strength of an optimisation.

**Scale.** `tools/langsys_survey.py` measured the host before the fix: of 581
installed faces, 290 register at least one LangSysRecord, 3031 (script,
language) records in all, 1203 of which differ from their default and **996 of
which differ in a feature tag this crate asks for, across 230 faces**. Moved
tags: `locl` 856, `ccmp` 90, `liga` 67, `calt` 28, `mark` 25. The survey's
feature list is pinned equal to the shaper's by
`otl::tests::the_survey_matches_the_shapers_feature_list`, so a number it
reports cannot quietly come to mean something else.

**Tested** by seven new unit tests over hand-built ScriptLists that
`fixture::script_list` cannot express (a script with no DefaultLangSys, a
script with named languages, a `requiredFeatureIndex`, a face registering only
a language's second candidate, and both orders of a face registering two of
them), by 20 tests over the BCP 47 mapping, and by `tools/harfbuzz_sweep.py`,
which grew a language field: each new corpus entry is a string already in the
corpus plus a tag, so a difference between the two halves is the language and
nothing else, and both halves map the tag with the same rules. The sweep's
buffer language is set *after* `guess_segment_properties`, and explicitly to
`""` for the language-less entries, because the guess otherwise fills it in
from the machine's locale and the run would pass or fail by where it was made.

**What the oracle caught.** The first version of this fix passed 521 unit
tests, was clippy-clean, and was wrong. The sweep found it in one run: `ro-MD`
disagreed with HarfBuzz on **345** faces where plain `ro` disagreed on 279, and
the 66-face gap was the bug. HarfBuzz's `hb_ot_tags_from_language` returns an
ordered list of up to `HB_OT_MAX_TAGS_PER_LANGUAGE = 3` candidate tags and asks
the face for each in turn; `gen_lang_tables.py` had deliberately kept only the
first of each list, on the reasoning that a language has one tag. Those 66
faces — `Candara.ttf` among them — register `('latn', 'ROM ')` and no `MOL `,
so HarfBuzz reached Romanian's comma-below `locl` for Moldavian through the
second candidate and we did not. After the generator was reworked to keep all
of them, the `ro-MD` bucket is 279: exactly `ro`'s, exactly the language-less
twin's, and entirely the pre-existing NFC divergence recorded in
`design-decisions.md` §410. Final sweep: 556 faces × 35 strings, 18235 agree,
reordered 0, misplaced 0.

This is the third bug the HarfBuzz oracle has found that a green unit-test
suite could not, and for the same reason every time: "this face has no glyph
/ no language system for that" is a *legal* answer, so no self-consistency
check can tell it apart from the truth. Only a second implementation can.

The original entry follows.

---

**What.** `otl::select` reads each ScriptRecord's DefaultLangSys and ignores
its LangSysRecords entirely. The per-language overrides — Turkish dotless `i`
under `TRK `, Serbian Cyrillic italic letterforms under `SRB `, Moldovan
comma-below under `MOL ` — are never reached, and a face whose *only* route to
a feature is a language system contributes nothing at all.

**Why it bites.** It is invisible until it is not. A Turkish reader gets the
wrong dot on `i`/`ı`; a Serbian reader gets Russian italics for бгпт. Both are
the kind of wrongness a native reader notices immediately and nobody else ever
does.

**Why it is filed rather than fixed.** There is nothing to select *with*.
Language is a property of the text's provenance, not of its characters — the
same Cyrillic codepoints are Serbian or Russian depending on who typed them —
so it cannot be derived the way script is. It needs a language carried on the
text down to `ScaledFont::shape`, which means an API change reaching the
toolkit and the locale system, neither of which has a language to hand yet.

**Proper fix.** Add an optional BCP 47 language to the shaping call, map it to
an OpenType language system tag (the registry is a fixed table, `tr` → `TRK `,
`sr` → `SRB `), and have `select` prefer that LangSysRecord over the
DefaultLangSys. Default stays "no language", which is what every shaper does
when not told and what this crate does now — so the change is additive and
cannot regress text that names no language.

**Reproduce.** `gsub::tests::a_feature_only_a_language_system_reaches_is_not_applied`
pins the current behaviour: a `locl` reachable only through `TRK ` yields no
`Substitutions` at all.

**Where.** `gui/font/src/otl.rs` — `select`, `LangSys`, and the module doc's
"What is not here".


## TD-FONT-DOES-NOT-HIDE-DEFAULT-IGNORABLES — RESOLVED 2026-08-15

**Resolved in two commits, because it was two bugs wearing one name.**

*Half one — erasing them* (`88ee69ca7`): `norm::ignorable` classifies the
character, `SubGlyph::ignorable` carries the answer and is cleared wherever a
`GSUB` lookup rewrites the glyph, and `ScaledFont::shape` replaces what is left
with the space glyph, or drops it where the face has none.

*Half two — stepping over them* (this commit): erasing an ignorable at the end
is not enough, because the lookups in between still saw it as a wall. `f ZWJ i`
did not ligate; a contextual alternate did not match across a soft hyphen. The
matcher now answers three ways rather than two, as HarfBuzz's does — hide,
*step over*, or consider — with the kind of ignorable and the kind of lookup
deciding which. See `design-decisions.md` §434 for the shape of that, and
`gui/font/src/skip.rs`'s `Joiners` for the table.

**Measured.** Host sweep, 556 faces × 60 strings: `differ` on `f\u200di` went
from 76 faces to 0, and `misplaced` from 331 to 170. Khmer probe: 45/45 before
and after, which is the point — the Indic-family features read the joiners
themselves and had to come through unchanged.

**The 170 that remain are a deliberate divergence, not a residue.** They are
every corpus string containing an ignorable, and in all of them the glyphs and
every *visible* glyph's position agree; what differs is the x of the erased,
zero-advance glyph itself. HarfBuzz spends a legacy `kern` on the right-hand
glyph's offset, so its erased glyph sits at the *unkerned* pen — 13 units
inside the following letter's image, for `a◌͏b` in Arial Rounded. We charge the
kern to the pair's left glyph, so ours sits exactly where the next glyph is
drawn. A caret asked to land on the ignorable's cluster wants ours. Recorded in
`design-decisions.md` §434; do not "fix" it without reading that first.

---

*The original entry follows, as filed.*

**What.** A handful of characters exist to instruct the shaper and are never
meant to be drawn: the zero-width joiner and non-joiner, the soft hyphen, the
bidi controls, the variation selectors, the byte-order mark. Once shaping is
over, HarfBuzz erases them — `hb_ot_hide_default_ignorables`, in
`hb_ot_substitute_post`, replaces each one's glyph with the face's `space`
glyph, or **deletes the glyph entirely** if the face has no space — and
`hb_ot_zero_width_default_ignorables`, during positioning, zeroes their
advances and x-offsets first. We do neither: `ScaledFont::shape` maps the
character through `cmap` like any other and returns whatever glyph came back.

**Symptom, measured.** The two strings the Khmer probe font disagrees on
(`gui/font/tools/khmer-corpus.txt`, the `\u17d2\u200d\u1781` and
`\u17d2\u200c\u1781` lines) are exactly this: HarfBuzz emits the space glyph
where we emit ZWJ's and ZWNJ's own glyphs. It is invisible in the host sweep
only because the built-in corpus has no string containing an ignorable that
the face also maps.

**Why it matters beyond the joiners.** This is crate-wide and
script-independent, and the joiner case is the *benign* one — a face that maps
ZWJ usually maps it to something blank anyway. The soft hyphen U+00AD is the
one that bites: fonts routinely map it to a real hyphen glyph, so a word
carrying a discretionary break renders with a hyphen sitting in the middle of
it whether or not the line broke there. The bidi controls and variation
selectors are the same shape of bug.

**One subtlety that is easy to get wrong.** HarfBuzz's predicate is
`(unicode_props() & UPROPS_MASK_IGNORABLE) && !_hb_glyph_info_substituted()` —
a character stops counting as ignorable the moment a GSUB lookup rewrites it,
because at that point the glyph is whatever the font asked for and is no
longer the control character. So the flag has to be *cleared on substitution*,
not merely tested at the end. And the set is HarfBuzz's own hard-coded list
(U+00AD, U+034F, U+061C, U+17B4–17B5, U+180B–180E, U+200B–200F, U+202A–202E,
U+2060–206F, U+FE00–FE0F, U+FEFF, U+FFF0–FFF8, U+1BCA0–1BCA3, U+1D173–1D17A,
U+E0000–E0FFF), *not* Unicode's `Default_Ignorable_Code_Point` property; using
the Unicode set would make the sweep disagree in the other direction.

**Proper fix.** A flag on `SubGlyph`, set in `scaled.rs`'s per-piece build loop
from the character, cleared at the three sites in `gsub.rs` that assign a
glyph id — `apply_single`, `apply_alternate`, the ligature path — and by
`apply_multiple`'s splice. Then in the loop that builds `out: Vec<ShapedGlyph>`
at the end of `shape`, zero the advance and offsets and substitute the space
glyph, or drop the glyph if the face maps no space. Corpus strings containing
a soft hyphen and the joiners go into `harfbuzz_sweep.py`'s built-in `CORPUS`
in the same change, so the fix is measured on all 556 host faces rather than
on the one probe font that happened to expose it.

**Where.** `gui/font/src/scaled.rs` — the per-piece loop that derives
`tab`/`klass`/`mark`/`indic` from each character, and the `out`-building loop
after it; `gui/font/src/gsub.rs` — `apply_single`, `apply_multiple`,
`apply_alternate` and the ligature path; `gui/font/tools/harfbuzz_sweep.py` —
`CORPUS`.


## TD-FONT-HAS-A-HANGUL-SHAPER-NOTHING-CALLS — ✅ FIXED 2026-08-15

**What.** `gui/font/src/hangul.rs` is a complete, tested transcription of
HarfBuzz's `preprocess_text_hangul` — 673 lines, 19 tests, all passing — that is
**not declared in `lib.rs`** and therefore compiles nowhere and changes no
output. It was parked mid-task on an explicit halt, at the point where it worked
in isolation but was not yet connected.

**Why it is parked rather than either finished or deleted.** The connection is
all-or-nothing, and the half of it that was written first is a regression on its
own. Wiring the shaper means telling `norm::pieces` to stop normalizing Hangul —
HarfBuzz's Hangul shaper sets `HB_OT_SHAPE_NORMALIZATION_MODE_NONE` precisely
because composing first destroys the distinction the shaper reads. But `pieces`
composing `<L,V,T>` to a syllable is currently the *only* thing that makes
Korean render at all on the ordinary Korean text font, which ships the 11,172
precomposed syllables and no jamo. Exempt Hangul from normalization without the
shaper in place and that font draws three missing-glyph boxes where it used to
draw one correct syllable. So the `norm.rs` half was reverted and the module
kept: a tested, inert file loses nothing, whereas a half-wired one is worse than
neither.

**The four edits that connect it**, in the order they have to happen:

1. `norm.rs` — thread a private `enum Hangul { Normalize, LeaveAlone }` through
   `decompose_once`, `compose_pair`, `decompose_into` and `compose`; split `nfc`
   into `nfc` (which passes `Normalize`, because NFC is NFC and a question about
   *text* must get that answer) and a private `normalize(text, hangul)`. `pieces`
   then calls `normalize(text, Hangul::LeaveAlone)`, and `split_undrawable` calls
   `decompose_once(ch, Hangul::LeaveAlone)` — the latter because a syllable
   `hangul::preprocess` declined to split has been declined on grounds
   `split_undrawable` cannot see, namely that the face has no jamo either. Three
   call sites in `norm.rs`'s own tests need the new argument.
2. `gsub.rs` — add `b"ljmo"`, `b"vjmo"`, `b"tjmo"` to `FEATURES` with `LJMO`,
   `VJMO`, `TJMO` bit constants, and a `SubGlyph::jamo(gid, cluster,
   Option<Jamo>)` constructor that ORs the one jamo bit and **clears `CALT`**.
   Clearing `calt` is not an optimization: Noto Sans CJK and Source Han Sans file
   all of their jamo lookups under `calt`, and HarfBuzz's `setup_masks_hangul`
   turns it off for every L/V/T so those lookups cannot fire twice.
   `the_masks_match_the_feature_list` has to keep passing.
3. `scaled.rs::shape` — call `hangul::preprocess` immediately after
   `norm::pieces`, with `has_glyph = |ch| self.face.glyph_index(ch).is_some()`
   and `zero_width = ` has-glyph *and* zero horizontal advance; then choose
   between `SubGlyph::cursive` and `SubGlyph::jamo` in the piece loop on
   `hangul::is_jamo(ch)`. Guard the whole thing with `hangul::present` so a run
   with no Korean in it pays nothing.
4. `fallback.rs` — add `*b"hang"` to `NO_ZERO_WIDTH_MARKS` (the Hangul shaper's
   `zero_width_marks` is `NONE`) and **not** to `COMPLEX_SCRIPTS` (its
   `fallback_position` is `true`). Both lists are `is_sorted`-asserted.

**What it should buy.** 553 of the sweep's 892 remaining `differ` cases are the
single string `\u1100\u1161\u11a8` — jamo we compose to `각` and HarfBuzz leaves
as three glyphs. Expect `differ` 892 -> ~339. The residue after that is composed
Latin diacritics, which is a *different* and unsettled question: HarfBuzz
decomposes and recomposes against font coverage, which reverses the layering
`norm.rs`'s module doc deliberately chose (`nfc` pure Unicode, `fit_to_face` pure
font). That one is an operator question, not a bug.

**Where.** `gui/font/src/hangul.rs` (parked), `gui/font/src/lib.rs` (the missing
`mod hangul;`), and the three files named above. The reference is HarfBuzz's
`src/hb-ot-shaper-hangul.cc`.

**Resolution — 2026-08-15.** All four edits landed together with the missing
`mod hangul;`, and the prediction above held to the case. The HarfBuzz
differential sweep (556 host faces × 23 strings, 12,739 comparisons):

| bucket | before | after |
|---|---|---|
| `agree` | 11,847 | **12,400** |
| `differ` | 892 | **339** |
| `reordered` | 0 | 0 |
| `misplaced` | 0 | 0 |

`\u1100\u1161\u11a8` — all 553 of its cases — left the disagreement list
entirely, and nothing regressed into `reordered`/`misplaced`. `osfont` goes
from 482 to **501 passing tests**: the module's own 19 tests had never run
before, because a module that is not declared does not compile and therefore
does not test either. That is the sharper lesson here — "19 tests, all
passing" was a true statement about a file `cargo test` had never once
looked at.

Two notes on how the edits differ from the plan above. `gsub.rs`'s three new
feature tags are **appended** to `FEATURES` rather than inserted in tag order,
so that no existing bit constant shifts; the bits are `1 << 34/35/36`.
And `norm::nfc` lost its last production caller in the split, so it now
carries `#[cfg_attr(not(test), allow(dead_code, …))]` — it is kept deliberately
as the text-question half of the split (NFC is NFC), not as dead weight, and
the reason is written at its definition.

The residual 339 are exactly the composed-Latin-diacritics cases this entry
predicted (`\u1e09` 255, `\u212b` 57, `été` 10, …). They are **not** tracked
here as a bug; they are the layering question in `norm.rs`'s module doc, and
belong to the operator. See `open-questions.md`.
### [B] D-POSIX-SOCKET-META-WAS-NOT-SCOPED-TO-ITS-FD-TABLE — ✅ FIXED 2026-08-14

**Found while running the eighth audit pass**, not by looking for it:
`socket::tests::test_phase201_bind_port443_no_cap_eacces` failed once with
`ENOTSOCK` where `EACCES` was expected, then passed three runs in a row.

`SOCKET_META` (posix/src/socket.rs) is indexed by fd number, so it must have
exactly the same scope as the fd table it is keyed by. `fdtable` made its
storage **per-thread** on host builds (design-decisions.md §110) precisely
because libtest runs tests on parallel threads. `SOCKET_META` stayed a
process-global `static mut`, and the mismatch was reachable: two tests on
different threads each create a socket and, drawing from *separate* per-thread
fd tables, both get the same fd number `N` — near-certain, not unlikely, since
each thread's table starts empty. They then shared one `SOCKET_META[N]`, and
the first to `close()` wiped the entry the other was still using, whose next
call saw a live fd with no metadata and reported `ENOTSOCK` for a good socket.

Fixed by giving `SOCKET_META` the same `cfg`-split storage as
`fdtable::fd_store`. Six consecutive full runs clean afterwards.

Two things worth keeping from this. First, the `// SAFETY: Single-threaded
access.` comments on these accesses were **true on the target and false under
`cargo test`** — a safety comment that silently changes truth value with
`cfg` is worse than none, and `fdtable` had already learned this lesson
without the fix being propagated to the table keyed by its own indices.
Second, an intermittent failure at roughly one run in four is easy to
dismiss as noise when it appears in a test unrelated to what you are
changing; it was worth the ten minutes to chase.

### [B] D-POSIX-TIMED-WAITS-DID-NOT-VALIDATE-TV-NSEC — ✅ FIXED 2026-08-14

`pthread_cond_timedwait`, `pthread_mutex_timedlock` and `sem_timedwait`
accepted any `timespec` whatsoever. A `tv_nsec` of `1_000_000_000` or `-1` —
the classic result of adding a nanosecond offset without carrying into
`tv_sec` — should be `EINVAL` (glibc `valid_nanoseconds`, `include/time.h:517`);
instead it fell through to the deadline comparison, where a too-large
`tv_nsec` silently extended the wait by up to a second and a negative one made
the call return `ETIMEDOUT` immediately. Both are wrong in the direction that
hides the caller's bug. Separately, `mqueue::deadline_from_timespec` checked
`tv_nsec` but not `tv_sec < 0`, which the kernel's `timespec64_valid` rejects.

Fixed by adding `time::valid_nanoseconds` (glibc's predicate, verbatim) and
calling it from each site **at the position its own upstream uses** — eagerly
in `pthread_cond_timedwait` and `sem_timedwait`, lazily (contended branch
only) in `pthread_mutex_timedlock` — plus the missing `tv_sec` half in
`mqueue`. See the ninth-pass write-up under
`D-POSIX-NULL-POINTER-ERRNO-NEEDS-A-PER-FUNCTION-AUDIT` for why the three
placements differ and why the mqueue predicate is not the same predicate.

Seven tests pin the distinctions, including the two that would silently pass
under a naive "check it at the top of every function" fix:
`test_pthread_mutex_timedlock_uncontended_ignores_a_bad_deadline` and
`test_sem_timedwait_checks_the_deadline_before_the_fast_path`.

**Not fixed, because we do not have them:** `pthread_cond_clockwait`,
`sem_clockwait` and the `pthread_rwlock_{timed,clock}{rd,wr}lock` family are
unimplemented. When they are added they need the same predicate plus
`futex_abstimed_supported_clockid`, and the rwlocks check **eagerly** — see
the comment at `pthread_rwlock_common.c:286-291`.

---

### [B] TD-OILS-A-PROCESS-SUBSTITUTION-IN-A-BRACE-BODY-IS-NEVER-PERFORMED. bash runs `${z:-<(echo hi)}` and substitutes `/dev/fd/63`; osh yielded the nine characters `<(echo hi)` — 2026-08-14 — ✅ FIXED 2026-08-14

**Where it was:** `userspace/oils/src/lexer.rs`, [`Lexer::read_word_verbatim`],
which reads the operand, the pattern and the replacement of a `${ … }` and had
no `<`/`>` arm at all.

bash splits this construct across two files and osh had only one half of it.
**Part (A) — the parse** — is `parse_matched_pair` naming `<(`, `>(` and `$(` in
one breath (parse.y:5028) and sending all three through `parse_comsub`
(parse.y:5042), so a `${ … }` body's scan parses a process substitution where it
meets it, its syntax error is the enclosing unit's, and what survives is the
parse *re-printed*; see
`userspace/oils/tests/corpus/a-process-substitution-in-a-brace-body-is-parsed-where-it-is-met.sh`
and [`parser::procsub_reprints`]. **Part (B) — the performance** — is
`expand_word_internal` *running* it, and was this entry.

**The rule** is bash's quoting flag, not the position. `expand_word_internal`
reads a process substitution only when `if (string[++sindex] != LPAREN ||
(quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) || (word->flags & W_NOPROCSUB))`
lets it (subst.c:11079), so an **operand** runs one when the expansion is bare
and keeps the characters when it is double-quoted, a **pattern** and a
**replacement** run one either way (both are re-entered without
`Q_DOUBLE_QUOTES`), and a **subscript** or a **substring bound** never does
(`Q_DOUBLE_QUOTES|Q_ARITH`), so its arithmetic error names the characters.

**The fix.** [`Verbatim`] gained an `Arith` mode beside `Bare`, `Replacement`
and `Dquote` — identical to `Bare` in every other respect — and
[`Lexer::read_word_verbatim`] gained a `<`/`>` arm live in `Bare` and
`Replacement` only. On the parser side [`parser::verbatim_word_at`] picks the
lexer entry from a new `Frag` (`Word` or `Arith`), which is what a subscript and
the `' … '` runs inside it now pass. The body the arm reads is already the
*re-print* part (A) spliced in, which is what bash performs too: the token
buffer a `${ … }` scan leaves behind holds the re-print and nothing else.

No new expansion machinery was needed. The double-quoted operand was already
right — the splice puts the re-print into the text and its nested `$( … )` then
expands normally, so `"${z:-<(echo $(echo q))}"` is `<(echo q)` in both shells —
so the whole of part (B) was one liveness decision taken at lex time, which is
where osh decides quoting.

**The pre-existing inconsistency this closed.** The substring bound
(`${z:<(echo hi)}`, via [`parser::parse_slice_bounds`]) *did* perform the procsub
while the subscript beside it did not, so osh's two arithmetic contexts — which
bash expands identically — disagreed. The bound is tokenized rather than read
verbatim, so it has no `Verbatim` mode to set; [`parser::word_from_source`], its
only reader, now turns a `Seg::ProcSub` back into the characters it was read
from. Both contexts are on the same side now.

**Verified:** `a-process-substitution-in-a-brace-body-is-performed-unless-the-expansion-is-quoted.sh`,
27 cases across the five contexts. None of them prints a substitution's path —
bash names it `/dev/fd/N` and osh a temporary file — so each asks a question the
path does not answer: whether the text still begins `<(`, whether it names
something that exists, or what a `cat` of it reads.

**How it was found:** implementing part (A) — the eager parse and re-print of a
process substitution met by a `${ … }` body scan.

### [B] TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN. bash's `brace_gobbler` and its `${x@P}` re-read each meet a `<(` osh's do not — 2026-08-14 — ✅ FIXED 2026-08-14 (both halves, and the arithmetic-fragment residue)

Two residues of TD-OILS-A-PROCESS-SUBSTITUTION-IN-A-BRACE-BODY-IS-NEVER-PERFORMED
(above), left after both halves of it were done. Each is a *second* scan of the
same text — one that is not `parse_matched_pair` and not `expand_word_internal` —
which has a `<(` row of its own that osh's counterpart lacks. The `$(` spelling
of each already matches bash byte for byte, so in both the machinery is there
and only the row is missing.

**Where:** `userspace/oils/src/interp.rs`, [`Shell::gobbled_subs`]; and the
`${x@P}` re-read, `userspace/oils/src/parser.rs`, [`dquote_word_from_source`].

* **✅ FIXED 2026-08-14.** `echo "${z:-"<(fi)"}"` — bash reports
  `command substitution: line N+1: syntax error near unexpected token 'fi'`
  plus the tail of the physical line, where osh prints `<(fi)`. The agent is
  **`brace_gobbler`**, whose command-substitution row names all three spellings
  (`(c == '$' || c == '<' || c == '>') && text[i+1] == '('`, braces.c:675) and
  reaches `extract_command_subst` → `xparse_dolparen`, which *parses* the body
  and throws the result away. Two facts pin it down. The gobbler's `quoted`
  state does not nest and `${` opens none of its own (it is treated like `\{`),
  so the **inner** `"` is `c == quoted` and clears the state — which is why the
  row fires here and not in the plain `"${z:-<(fi)}"`, where parse.y has
  already answered. And it fires only where brace expansion runs: an argument
  or command word errors (`: "${z:-"<(fi)"}"`, `f "${z:-"<(fi)"}"`,
  `echo "${a["<(fi)"]}"`), while an assignment RHS — which is not brace-expanded
  — does not (`x="${z:-"<(fi)"}"` is silent). bash only ever *parses* it: with a
  body that does parse, `echo "${z:-"<(echo hi)"}"` prints `<(echo hi)` in both
  shells, so this is a diagnostic and not a missing expansion.

  What was missing was something to hang the row on. [`Shell::gobbled_subs`]
  walks the *parse* structurally, and here the tree is right to hold characters
  — the `<(` sits in a `" … "` run inside a double-quoted operand, where neither
  bash's expander nor osh's reads one — so no part was ever going to appear for
  it. The fix is therefore not another lexer mode but a text-level pass beside
  the structural walk, as `gobbled_backtick_subs` already is for a backquote:

  * `wordscan::gobbler_procsubs(s, dquoted)` — the same flat-state loop as
    `gobbler_readable`, reporting the index of each `<(`/`>(` met while `quoted`
    is 0. (`gobbler_readable` could not answer this: it reports the stretches the
    **`$(`** row fires in, which is `quoted == 0` *and* `quoted == '"'`, and the
    `<(` row is the first of those alone.) A `$( … )` is skipped whole rather
    than reported — that is the one spelling a part already stands for.
  * `Shell::gobbled_procsubs` — for each index, lex `$(` + the rest of the word
    with `parser::dquote_word_from_source` and take the resulting
    `CmdSubBody::Unread`. The two spellings reach the same
    `extract_command_subst`, so the swap is exact, and one lex settles the body,
    the remainder and whether there was a `)` at all. It is a *lex*, not the
    paren count `gobbler_readable` skips with, because `xparse_dolparen` is a
    real parse: a `(` inside a quoted run of the body is not a nesting level to
    it, and a count would carve `echo <(echo "(")` into a body that fails.
  * The two are merged by **remainder length**: every tail the gobbler's word
    carries is measured against the whole word (`unparse::gobbler_word`), so a
    longer one is an earlier meeting. That is what keeps the interleaving right
    where a word holds both — measured, `echo "${z:-'$(fi)'"<(for)"}"` reports
    the `$(fi)` and `echo "${z:-"<(fi)"'$(for)'}"` the `<(fi)`.
  * `Shell::has_gobbled_sub` — the cheap pre-test — gained a `WordPart::Literal`
    row, answering wide (any `<(`/`>(` in a literal under quotes) so the word
    reaches the scan that settles it.

  **Verified:** `userspace/oils/tests/corpus/a-process-substitution-a-brace-scan-meets-is-read-where-the-quoting-is-clear.sh`,
  29 rows, all matching bash 5.2.37 — including the parity (`"${z:-"a"<(fi)"b"}"`
  is a *parse* error, `"${z:-"${y:-"<(fi)"}"}"` is silent), the `set +B` gate, the
  words brace expansion does not reach (assignment RHS, `case` word, here-doc
  body), the read happening before expansion (`z=Z`, `${z:+…}`), and the `declare
  -f` re-print.
* **✅ FIXED 2026-08-14 for the double-quoted operand** (`${z:-…}`, `${z:+…}`,
  `${z:=…}`, `${z:?…}` and the plain `${z-…}` family) — which is the position
  the report named, and the only one a `${x@P}`/`PS4` re-read reaches with the
  quoting bash's own expansion declines a process substitution under. The
  remaining positions are a residue of their own, logged at the end of this
  bullet. Original report: `x='${z:-<(fi)}'; echo "${x@P}"` — bash's `extract_dollar_brace_string`
  (subst.c:1881-1950) has a `<(` row of its own and recurses into it with a real
  parse, so the `@P` re-read is a `bad substitution` and the text is printed
  unchanged; osh splices the re-print and prints `<(fi)`.

  **Measured against bash 5.2.37 (2026-08-14).** The row behaves as the `$(`
  row beside it in every respect: `A${z:-<(fi)}TAIL` and `A${z:-$(fi)}TAIL`
  give byte-identical output, down to the quoted remainder `` `fi)}TAIL' ``
  and the `line 2` numbering `xparse_dolparen` gives an unread body. It is the
  scan's row and not the string's — `x='a<(fi)b'` is silent — and it is reached
  only where the scan's own quoting allows: `"<(fi)"` (double-quoted),
  `'<(fi)'` (single-quoted, `skip_single_quoted`) and `\<(fi)` are all silent
  and print their text. A body that parses is silent too and is *not*
  performed: `A${z:-<(echo A >&2)}B` prints `A<(echo A >&2)B` and no `A` on
  stderr.

  osh already matched on six of those shapes. What it got wrong:

  | written (as `x`, then `echo "${x@P}"`) | bash | osh (before) |
  |---|---|---|
  | `A${z:-<(fi)}TAIL` | reports, `bad substitution`, text | `A<(fi)TAIL` |
  | `A${z:-${y:-<(fi)}}B` | reports (nested body too) | `A<(fi)B` |
  | `A${z:-p<(fi)q$(for)r}B` | reports the **`<(fi)`** | reports the `$(for)` |
  | `A${z:-<(fi}B` | `unexpected EOF`, `bad substitution`, text | runs `fi}` — `command not found` |

  All but the last now match. The last is a *different* defect that the `$(`
  spelling has identically — see
  TD-OILS-AN-UNCLOSED-SUBSTITUTION-IN-AN-UNREAD-BRACE-BODY-IS-RUN-INSTEAD-OF-REFUSED
  below — so it was left alone here rather than fixed twice.

  **Why it was not a two-line change.** The `<(` span *is* already collected —
  `Lexer::read_dollar_brace` has the row (lexer.rs:7069) and records a
  `CmdSubSpan` with `SubOpen::Proc`, its `src`, its `range` and
  `SubBody::Unread`. What is missing is a [`WordPart`] for
  [`Shell::brace_scanned_subs`] to walk to: `procsub_reprints`
  (parser.rs:6288) splices a re-print only for a `SubBody::Eager` span, and the
  re-lex that carves the operand out of the body (`read_word_verbatim`) leaves
  a `<(` as characters on purpose. So for an *unread* body the process
  substitution survives only as text in a `WordPart::Literal`.
  `arith_unread_subs` is the shape of the answer for the arithmetic scan, and
  it excludes this spelling deliberately (parser.rs:6233-6240).

  Two things make the obvious fixes wrong, both measured above:

  * **The remainder runs past the `}`.** `` `fi)}TAIL' `` and
    `` `fi)}B${y:-<(for)}C' `` are the rest of the *whole re-read string*, not
    of the `${ … }`. So a text scan confined to the brace's own source (the
    only text [`Shell::brace_extent_scan`] is handed) cannot build the part's
    `tail`, and the `$( … )` spelling gets its own from
    `unparse::attach_comsub_tails`, which runs over the assembled word in the
    parser.
  * **It must interleave with the `$(` spelling**, in the order the one scan
    meets them — hence the `p<(fi)q$(for)r` row above.

  Reusing [`CmdSubBody::Unread`] for the synthesized part is safe for the
  *read* (the diagnostic quotes the body's remnant, never the delimiter, so a
  `<(` and a `$(` in this position are byte-identical) but not for anything
  that re-prints or *runs* one — `interp.rs:34302` performs an unread body, and
  a process substitution here is never performed. So either the part carries
  its spelling (a new field on `CmdSubBody::Unread`, two construction sites and
  one printer, plus the run site taught to refuse) or it is synthesized late
  enough that it can never escape into a print or a run — which is what
  `Shell::gobbled_procsubs` does for the `brace_gobbler` half above, and the
  reason that one could be done without touching the AST.

  **What was done.** The first of the two: the part carries its spelling, which
  makes both blockers vanish rather than needing to be worked around.

  * `ast::SubDelim { Dollar, ProcIn, ProcOut }`, with `bytes()` (the delimiter
    as written) and `is_performed()` (true only for `Dollar`). Recorded on
    `CmdSubBody::Unread` and on the lexer's `SubBody::Unread`. Only the unread
    form needs it: a body a parser *read* is a `CmdSubBody::Parsed` for `$(`
    and a `WordPart::ProcSub` for the other two, so those two shapes already
    tell the spellings apart.
  * `Lexer::read_word_verbatim` gained a `<(`/`>(` row for `Verbatim::Dquote`
    **when the text is unread** (`self.here_text`), emitting
    `Seg::CmdSub(body, close, SubBody::Unread { delim })`. The existing
    `Verbatim::Bare | Verbatim::Replacement` row above it is untouched — those
    fragments really do *perform* the substitution, measured:
    `x='A${z/p/<(echo hi)}B'; echo "${x@P}"` prints a `/dev/fd/N` in bash.
  * `unparse.rs` prints the body back in `delim.bytes()`, and
    `Shell::command_sub_body` returns that text instead of running anything
    when `!delim.is_performed()`.
  * The backslash arm of the same loop takes a `\<(`/`\>(` into the literal
    run, because the *scan* that produced this text honours a backslash
    whatever follows it (`extract_dollar_brace_string`'s `case '\\'`,
    subst.c:1899) while the operand's own dquote read does not. `A${z:-\<(fi)}B`
    prints `A\<(fi)B` and reports nothing.

  Both blockers then answer themselves: the `tail` is filled by
  `unparse::attach_comsub_tails` over the whole assembled word (so it runs past
  the `}`, giving `` `fi)}TAIL' ``), and the interleaving is
  `Shell::brace_scanned_subs`'s existing left-to-right walk.

  **Verified:** `userspace/oils/tests/corpus/a-process-substitution-a-brace-re-read-meets-is-read-like-the-dollar-spelling.sh`,
  22 rows, all matching bash 5.2.37 — the byte-identity with the `$(` spelling,
  both interleavings, the nested body, the not-performed rows (including
  `>(cat)` and a body writing to stderr, quoted and unquoted), the read
  happening before the operand is chosen (`z=Z`, `${z:+…}`), the four shields
  (unbraced text, `" … "`, `' … '`, backslash), the stepped-over subscript, and
  the `PS4` spelling of the same re-read.

* **✅ FIXED 2026-08-14** (every position but the arithmetic one; that one
  closed later the same day, at the end of this bullet)**.** The row
  was wired for the double-quoted **operand** only, and bash's scan reads the
  whole `${ … }` body — it walks characters and knows nothing of the `#`, `/`
  or `^^` it has already passed — so every other fragment wanted the same row:

  | written (as `x`, then `echo "${x@P}"`) | bash | osh before |
  |---|---|---|
  | `A${z#<(fi)}B` (pattern) | reports ×2, `bad substitution`, text | right text, **no diagnostics** |
  | `A${z/p/<(fi)}B` (replacement) | reports ×2, `bad substitution`, text | right text, **no diagnostics** |
  | `A${z^^<(fi)}B` (case pattern) | reports ×2, `bad substitution`, text | right text, **no diagnostics** |
  | `A${z:0:<(fi)}B` (offset) | reports ×2, `bad substitution`, text | `AB` |

  The `$( … )` spelling was right in all four (measured), so again only the row
  was missing. It was harder than the operand's, because in these positions the
  substitution is *both* read for its extent **and** performed — a replacement
  really does expand to `/dev/fd/N`, measured — so the part could not simply be
  the non-performed `CmdSubBody::Unread` the operand's is.

  **What was done.** The split `CmdSubBody` already makes between a body a
  parser read and one only a scan read is now made for the process-substitution
  part too, so one part answers for both halves:

  * `ast::ProcSubBody` — `Parsed(Program)` or `Unread { src, tail, closed }` —
    replaces the bare `Program` in `WordPart::ProcSub`.
  * `lexer::ProcRead` (`Eager` / `Unread { closed }`) rides on `Seg::ProcSub`;
    the `Verbatim::Bare | Verbatim::Replacement` arm of
    `Lexer::read_word_verbatim` picks it from `self.here_text`, and now
    tolerates a missing `)` exactly as the `$(` spelling does.
  * `parser::seg_to_part` parses only an eager body. An unread one is carried
    as text, because its read belongs to the scan and happens later, from
    where a failure is `bad substitution` rather than a script syntax error.
  * `unparse`: an unread body prints back as written, and joins
    `attach_comsub_tails` so it gets the same remainder the `$(` spelling does.
  * `interp`: `Shell::brace_scanned_subs_slice` collects it,
    `Shell::extent_read_of_subs` reads it through the same
    `comsub_reparse_read`, and the new `Shell::proc_sub_body` parses-then-
    performs at expansion — only reachable if that read succeeded.

  **Verified:** corpus case
  `a-process-substitution-a-brace-re-read-meets-is-read-wherever-in-the-braces-it-sits.sh`,
  21 rows, IDENTICAL against bash 5.2.37.

  **✅ The arithmetic fragment, 2026-08-14.** Deferred at first, because osh
  diverged over `<` in a bound before any process substitution was written at
  all (`${z:1<(2)}` is `bcdef` in bash and was an `operand expected` in osh);
  that was fixed as
  TD-OILS-A-LESS-THAN-IN-A-BRACE-ARITHMETIC-FRAGMENT-LOSES-ITS-LEFT-OPERAND,
  and this row followed.

  It was **not** simply `Verbatim::Arith`'s row, as the deferral assumed. A
  subscript shares that mode and must *not* get it: bash's scan steps over a
  subscript whole (`skip_matched_pair` from the `[`), so `${z[<(fi)]}` never
  offers its body to `extract_command_subst` and is an `operand expected` —
  which osh already matched. A bound is walked in the open. So the mode split
  in two: `Verbatim::Bound` / `Frag::Bound`, reached by `lex_bound_verbatim`
  and `parser::word_bound_from_source_at`, identical to `Arith` in every
  respect but that it takes `Dquote`'s unread-`<(` arm. That is the whole
  change — the arm was already written for the operand, and the read/perform
  split it produces (`SubBody::Unread`) is exactly a bound's: read for its
  extent by the scan, never performed, because `Q_DOUBLE_QUOTES|Q_ARITH` is
  what stops `expand_word_internal` (subst.c:11079).

  No interp-side work was needed: `unparse::nested_parts` already classifies
  `ParamSubstr`/`ArraySlice` bounds as `Nested::Operand`, so
  `Shell::brace_scanned_subs_slice` was already descending into them.

  **Verified:** 14 further rows in the same corpus case (the bound in offset
  and length position, `${a[@]:…}` and `${@:…}`, the `@P` and `PS4` spellings,
  the three quotings that shield it, and the well-formed `${z:<(echo 1)}` that
  reaches the evaluator as characters), IDENTICAL against bash 5.2.37.

**How it was found:** implementing the entry above.

### [B] TD-OILS-AN-UNCLOSED-SUBSTITUTION-IN-AN-UNREAD-BRACE-BODY-IS-RUN-INSTEAD-OF-REFUSED. `x='A${z:-$(fi}B'; echo "${x@P}"` runs `fi}` where bash reports `bad substitution` — 2026-08-14 — ⚠️ OPEN

A `$( … ` with no `)` inside a `${ … }` written in text no parser read — a
`${x@P}` re-read, a `PS4`, a here-document body. bash reads the extent with
`xparse_dolparen`, which fails at end of input; `si` is left past the end of the
string, so the brace never closes, so `parameter_brace_expand` reports
`bad substitution` naming the whole text and prints the text unchanged. Nothing
is run. osh gets the *first* diagnostic right and then runs the body anyway:

```sh
x='A${z:-$(fi}B'; echo "${x@P}"
# bash: command substitution: line 3: unexpected EOF while looking for matching `)'
#       line 1: A${z:-$(fi}B: bad substitution
#       A${z:-$(fi}B
# osh:  command substitution: line 3: unexpected EOF while looking for matching `)'
#       line 1: fi}: command not found
#       A

x='A${z:-$(echo hi}B'; echo "${x@P}"
# bash: … unexpected EOF …; … bad substitution; A${z:-$(echo hi}B
# osh:  … unexpected EOF …; Ahi}
```

Both spellings are affected identically — `<(fi}` behaves exactly as `$(fi}`,
which is the point: the delimiter is not what is wrong here.

**Where:** `userspace/oils/src/interp.rs`, [`Shell::extent_read_of_subs`]
(~29622) and [`Shell::run_abandoned_extent`]. The scan classifies the failed
read as `ExtentRead::Abandoned { body, rest }` and hands the body on to be run.
That classification is *right* for an abandoned extent bash really does run on
— it is `extract_command_subst`'s no-`)` path with the `jump_to_top_level`
suppressed — but wrong when the caller is the brace scan, because there the
unclosed read is also what stops the `}` from ever being found, and the
`bad substitution` that follows pre-empts the run.

**Proper fix:** distinguish the two callers. `extent_read_of_subs` should
report the abandonment to the brace scan (so `brace_extent_scan` fails the
whole `${ … }` and takes the `bad substitution` path with the source text)
rather than letting the body reach `run_abandoned_extent`. The `closed: false`
flag on `CmdSubBody::Unread` already names exactly this shape, so the test is
to hand.

**How it was found:** measuring the `<(` row of
TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN
against its `$(` twin, which turned out to be wrong the same way.

### [B] TD-OILS-A-LESS-THAN-IN-A-BRACE-ARITHMETIC-FRAGMENT-LOSES-ITS-LEFT-OPERAND

**Status:** ✅ FIXED 2026-08-14. Found 2026-08-14, measured against bash 5.2.37.
The cause turned out to be wider than the title: the two bounds were
**tokenized as a command** rather than read as arithmetic, so `<` was only the
most visible of the operators being lost. See "The fix" at the end.

A `<` in the offset or length of `${z:o:l}` swallows everything to its left.
The same expression inside a plain `$(( ... ))` is fine, so this is the brace
fragment's own reading of the text, not the arithmetic evaluator's:

| written | bash | osh |
|---|---|---|
| `z=abcdef; echo "${z:1<(2)}"` | `bcdef` | `z: <(2): syntax error: operand expected` |
| `z=abcdef; echo "${z:0:1<(2)}"` | `a` | same error |
| `echo $(( 1<(2) ))` | `1` | `1` |

bash reads `1<(2)` as `1 < (2)`, which is `1`, so the offset is 1. osh
evaluates `<(2)` alone -- the `1` is gone by the time the evaluator sees the
expression, which is what the quoted error token shows.

**Where:** `userspace/oils/src/lexer.rs`, the `Verbatim::Arith` path of
[`Lexer::read_word_verbatim`], and whatever splits a `${z:o:l}` body into its
two fragments in `userspace/oils/src/parser.rs`. The `<` is being taken for
something other than a comparison operator -- most likely a fragment boundary.

**Proper fix:** treat `<` in an arithmetic fragment as the comparison operator
it is, so the whole fragment reaches the evaluator. A `<(` there is *not* a
process substitution to be performed either -- measured, `${z:0:<(echo 1)}` is
an `operand expected` in bash with the characters `<(echo 1)` standing as the
error token, which osh already matches.

**Blocked, and then unblocked (same day):** the arithmetic-fragment row of
TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN.
bash's `${ ... }` scan reads a `<( ... )` in an arithmetic fragment exactly as it
reads one anywhere else in the body -- `x='A${z:0:<(fi)}B'; echo "${x@P}"`
reports the parse twice and then `bad substitution`, where osh printed `AB` --
but a corpus row for it would have been measuring this bug instead, so the
corpus case
`a-process-substitution-a-brace-re-read-meets-is-read-wherever-in-the-braces-it-sits.sh`
left that position out and said so. The fix below removed the obstacle, and the
rows went in the same day: that case now measures a bound in seven further
positions.

**How it was found:** measuring where bash's brace scan reads a `<( ... )`,
while checking whether the `Verbatim::Arith` fragments needed the same row as
the pattern and replacement ones.

**The fix (2026-08-14).** `parse_slice_bounds`
(`userspace/oils/src/parser.rs`) read each bound with `word_from_source`, which
called `tokenize(...)` — a *command* tokenizer — and then joined the surviving
`Tok::Word`s with a literal space. So every operator character was claimed by
the tokenizer instead of reaching the evaluator, and whatever it could not make
a word of was silently dropped. `<` was merely the case that produced an IO
number and a redirect. The rest, all measured against bash 5.2.37 with
`z=abcdef`:

| written | bash | osh, tokenized |
|---|---|---|
| `${z:1<2}` | `bcdef` | `cdef` — `1<` taken for a redirect |
| `${z:1>2}` | `abcdef` | `cdef` — likewise |
| `${z:1<=2}` | `bcdef` | `=2: operand expected` |
| `${z:1 < (2)}` | `bcdef` | `1 2: syntax error` |
| `${z:1;2}` | `;2: invalid arithmetic operator` | `1 2: syntax error` |
| `${z:1&2}` | `abcdef` | `1 2: syntax error` |
| `${z:3|2}` | `def` | `3 2: syntax error` |
| `${z:1&&2}` | `bcdef` | `1 2: syntax error` |
| `${z:1)}` | `1): syntax error in expression` | silently `abcdef` |

Both bounds now go through `word_subscript_from_source_at` — the very reader an
array subscript uses, which is `verbatim_word_at(..., Frag::Arith)` plus
`attach_subscript_reads`. The two arithmetic fragments therefore no longer
disagree with each other, which is what `attach_subscript_reads`'s own doc
comment had been asking for.

Two further defects of the same splitter were found while measuring it, and are
fixed in the same change:

* **Which colon cuts.** bash does not `strchr` for the `:`; `skiparith`
  (subst.c) skips one `:` for every `?` seen, and counts nothing at all inside
  a `( … )`. `${z:1?2:3}` is `cdef` (the whole text is the offset) while
  `${z:1?2:3:1}` is `c`; `${z:1?1?2:3:4}` is `cdef`, two `?` swallowing both
  colons; `${z:(1?2:3):1}` is `c`. osh split on the first `:` unconditionally
  and so reported `` `:' expected for conditional expression `` for all of
  these. Now `slice_split_colon` implements the rule.
* **An empty bounds text.** `${z:}` is `${z:}: bad substitution` in bash, and
  uniformly so — `${@:}`, `${*:}`, `${a[@]:}`, `${a[1]:}` and an unset
  parameter all report it. osh printed the whole value. It is the *text* that
  must be non-empty, not what it expands to: `${z:$e}` with `e=` is `abcdef`.
  `parse_slice_bounds` now returns `None` for an empty text and each of its
  three call sites turns that into `WordPart::BadSubst`.

Verified by the corpus case
`a-slice-cuts-its-bounds-with-skiparith-and-reads-each-as-arithmetic.sh`
(75 rows, IDENTICAL), the lib suite and a full sweep.

**Unblocked, and then done (same day):** the arithmetic-fragment row named
under "Blocks" above was the only thing left of
TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN,
and it is now closed there. It was a separate row from this entry's — after
this fix `${z:1<(2)}` evaluated correctly but `x='A${z:0:<(fi)}B'; echo
"${x@P}"` still printed `AB`, where bash reads the body for its extent and
reports `bad substitution`. It turned out **not** to be the `Verbatim::Arith`
row this entry's title suggested, because the *subscript* shares that mode and
must not get it: bash's `${ … }` scan steps over a subscript whole
(`skip_matched_pair`), so `${z[<(fi)]}` never offers its body to the scan and
is an `operand expected` in bash — which osh already matched. Only a bound is
walked in the open, so `Frag::Arith` split in two and the new `Frag::Bound`
took the row. See that entry for the change.

### [B] TD-OILS-AN-UNBALANCED-PAREN-IN-A-SLICES-BOUNDS-IS-AN-ARITHMETIC-ERROR-NOT-A-BAD-SUBSTITUTION

**Status:** ✅ FIXED 2026-08-14. Found 2026-08-14, measured against bash 5.2.37.
The fix turned up a second rule of the same walk, fixed with it — see "The fix"
at the end.

`skiparith` (subst.c) balances parens while looking for the colon that cuts
`${x:off:len}` in two, and an unbalanced `(` makes it run off the end. bash
then reports that as a **bad substitution** naming the whole bounds text, before
either bound is evaluated. osh implements the balancing (that is what makes
`${z:(1?2:3):1}` cut in the right place) but not the complaint, so the text
reaches the evaluator and produces an arithmetic diagnostic instead:

| written | bash | osh |
|---|---|---|
| `${z:(1}` | ``bad substitution: no closing `)' in (1`` | ``z: (1: missing `)' (error token is "1")`` |
| `${z:(1:2}` | ``… no closing `)' in (1:2`` | ``z: (1: missing `)'`` — and it cut at the colon |
| `${z:((1:2}` | ``… no closing `)' in ((1:2`` | likewise |
| `${z:1+(2}` | ``… no closing `)' in 1+(2`` | ``z: 1+(2: missing `)'`` |
| `${a[@]:(1}` | ``… no closing `)' in (1`` | arithmetic error |
| `${@:(1}` | ``… no closing `)' in (1`` | arithmetic error |

Both are rc=1, so only the message differs — but the message differs in class,
not just wording: bash's is the DISCARD-class `bad substitution` family, raised
by the cut, and it names the bounds text rather than the parameter.

Three things scope it precisely, all measured:

* It is the **whole bounds text** that is checked, once, before the cut — the
  message quotes `(1:2` entire, the colon never having split it.
* It is only the text the *cut* walks. Once a colon has been found with the
  depth back at zero, an unbalanced `(` in the length is an ordinary arithmetic
  error: `${z:0:(1}` is ``z: (1: missing `)'`` in bash too, and osh matches.
* A stray `)` at depth zero is not an error at all: `${z:)1}` is
  `)1: syntax error: operand expected` in both.

**Where:** `userspace/oils/src/parser.rs`, `slice_split_colon` — which already
tracks the depth and would only need to report a non-zero one at the end — and
its three call sites in `parse_braced_param_in`, which currently turn the
`None` that means "empty bounds" into `WordPart::BadSubst(raw)`.

**Proper fix:** `slice_split_colon` reports the unbalanced case distinctly from
the empty one, and the call sites raise ``bad substitution: no closing `)' in
<bounds text>``. That message shape already exists in
`userspace/oils/src/interp.rs` (`b"bad substitution: no closing `)' in "`,
~35600) but it names the whole *word*, whereas this one names the bounds text
only, so it needs its own carrier on the word part rather than a reuse of
`BadSubst`, whose printer names `${…}` entire.

**Blocked:** one row of the corpus case
`a-slice-cuts-its-bounds-with-skiparith-and-reads-each-as-arithmetic.sh`,
which said so in its header and left the shape out. Now measured there.

**How it was found:** measuring bash's slice bounds exhaustively while fixing
TD-OILS-A-LESS-THAN-IN-A-BRACE-ARITHMETIC-FRAGMENT-LOSES-ITS-LEFT-OPERAND. It
was the last of four divergences that measurement turned up, and the only one
not fixed there.

**The fix (2026-08-14).** Two things, because measuring the first turned up the
second.

**(1) The complaint.** `slice_split_colon` now returns the depth it ended at
beside the split index, `parse_slice_bounds` carries a non-zero one as
`SliceBounds::unclosed`, and both `WordPart::ParamSubstr` and
`WordPart::ArraySlice` gained an `unclosed: Option<Str>` field for it. It is a
field on the operator rather than a `WordPart::BadSubst`, because *where* it is
raised is the whole of what distinguishes the two: `${z:}` is a bad
substitution even for an unset parameter, while `${u:(1}` with `u` unset is
silently empty. So the check sits exactly where the offset would have been
evaluated — `Shell::slice_bounds_unclosed`, called from `scalar_slice`,
`assoc_slice` and the indexed path of `slice_elements_resolved`, each after its
own "nothing to measure" exit. Every ordering measured lines up: an empty
array, an empty `$@`, `set -u`, and a set-but-empty scalar (which *does* report,
having one position).

`no_longjmp_on_fatal_error` — `Shell::prompt_expanding` — **suppresses** the
complaint rather than rewording it, so under `${x@P}` or `PS4` the characters go
on to the evaluator and the arithmetic error is what comes out. That is the
`if (no_longjmp_on_fatal_error == 0)` guard the report sits behind, and it is
why osh's *old* answer was right in those two contexts and only those two.

**(2) The walk is quote-aware.** Measuring (1) showed the walk steps over a
`' … '` run, a `" … "` run and a backslash-escape whole — all three counters
included, not just the paren one. `${z:"1:2"}` does not split (the evaluator
meets `1:2` as one bound and says so), `${z:1"?"2:3}` does split (the quoted `?`
buys no colon), and `${z:0"("}`, `${z:0'('}`, `${z:0\(}` and `${z:(1"("2)}` are
all balanced. osh's walk saw none of that, so before this fix it both cut in the
wrong place and complained where bash did not. Note this is about the *walk*
only: the quote characters stay in the bound, and the arithmetic reading each
half is given removes them (or does not — a `' … '` keeps its second reading).

The walk is over the text **as written**, which the same measurement pins down
from the other side: `p="("; ${z:$p 1}` and `${z:$(echo "(1")}` are ordinary
arithmetic errors, each being balanced as written however unbalanced its value.

**Verified:** 37 further rows in
`a-slice-cuts-its-bounds-with-skiparith-and-reads-each-as-arithmetic.sh`, the
lib suite and a full sweep.

### [B] TD-OILS-THE-WAIT-NO-OPERANDS-CORPUS-CASE-IS-FLAKY-UNDER-A-FULL-SWEEP. The job holding `$!` is not spared, once per many sweeps — 2026-08-14 — OPEN

**Where:** `userspace/oils/tests/corpus/wait-with-no-operands-and-a-job-that-just-ended.sh`,
the group "only the last one backgrounded is spared", against
`Shell::builtin_wait`'s operand-less arm and `Shell::drain_jobs`
(`userspace/oils/src/interp.rs`).

**What — and this time the whole row was captured.** One full
`scripts/osh-bash-diff.py` sweep came back `654 matched, 0 waived, 1 failed`
with **one line** of the case different, everything else in it identical:

```sh
( exit 3 ) & ( exit 4 ) & sleep 0.4; wait; echo "  noargs=$?"
VAR=stale; wait -n -p VAR; echo "  n=$? $(pvar)"
```

| | bash 5.2.37 | osh (this sweep) |
|---|---|---|
| `noargs=` | 0 | 0 (agreed) |
| `n=` | `4 a pid` | **`127 unset`** |

So osh had nothing left to report where bash still had the last-backgrounded
job. Re-run on its own immediately after: `1 matched, 0 waived, 0 failed`.
Saved report:
`target/dvscratch/corpus-failures/20260814-145703/wait-with-no-operands-and-a-job-that-just-ended.txt`.

**What a 127 requires, read out of the code rather than guessed.** The spare is
`builtin_wait`'s operand-less arm: after `drain_jobs`, every job with a status
is marked `notified` *except* the one whose pid is `last_bg_pid`, and
`cleanup_dead_jobs` then drops exactly the notified ones. But `drain_jobs`
itself marks `notified` for every job it *waited for*, and it waits for any job
not already in its `known` snapshot — `known` being the jobs whose `exit_seen`
was set **before** the wait was reached. So the spare survives only when the
`$!` job's `exit_seen` was already set, which the unit-boundary
`cleanup_dead_jobs` does for a job that is both finished and older than
`JOB_EXIT_NOTICE_GRACE` (20 ms). A 127 means that did not happen for the `$!`
job specifically: had it been the *other* job that was late, `drain_jobs` would
have waited that one and the spare would still stand.

**The margin is not thin, which is what makes this odd.** Both shells were
measured at four margins (`build/pgS.sh`), and they agree exactly:

| `sleep` before the `wait` | bash | osh |
|---|---|---|
| none | `127 unset` | `127 unset` |
| 0.01 | `4 a pid` | `4 a pid` |
| 0.05 | `4 a pid` | `4 a pid` |
| 0.4 | `4 a pid` | `4 a pid` |

The flip is between 0 and 0.01, so the case's `sleep 0.4` is a ~40x margin — not
the ~1x margin that
TD-OILS-THE-COMPGEN-JOB-CORPUS-CASE-IS-FLAKY-UNDER-A-FULL-SWEEP turned out to
be. **Do not assume the same diagnosis and just widen the sleep.**

**Loads that do NOT reproduce it — do not spend the time again.** The job is
thread-backed, not a process (`( exit 4 ) & echo $!` prints the synthetic
`900000`, where `sleep 0.4 &` prints a real pid), so both of the obvious
starvation stories were tried and neither bit:

- 20 serial runs of the group alone: clean.
- 119 runs of the group at 8-way concurrency: clean.
- 64 runs of the *whole case* at 8-way concurrency: clean.
- 36 runs under a process-spawn storm (6 loops spawning `osh -c :` and
  `bash -c :` back to back, this host's documented ~200-290 ms spike source):
  clean.
- 30 runs under CPU saturation (24 busy-loop processes on 12 cores): clean.

Probes are `build/repro-wait.sh` (the group), `build/repro-wait2.sh` (the whole
case), `build/spawnstorm.sh`, `build/cpuburn.py` — all in the gitignored
`build/`, so re-create them from this entry if they are gone.

**Proper fix.** Unknown, and deliberately not guessed at. The next sighting
should establish which of the two conditions failed — whether the `$!` job's
body was genuinely unfinished at the unit-boundary poll, or whether the poll did
not run — by instrumenting `poll_jobs` to record, per job, `is_finished` and
`born_at.elapsed()` at each call, and dumping that when `wait -n` answers 127.
That distinguishes "the thread really was 400 ms late" from a bookkeeping fault,
and only the first is a case-margin problem.

**Impact.** An intermittently red sweep, which is the gate on every commit —
and the sweep takes ~19 minutes, so a re-run to disambiguate is expensive.

**Sighting 2026-08-14, in the *unit* suite, and fixed there.**
`interp::tests::wait_n_ignores_a_job_whose_status_was_already_reported` failed
once under `cargo test -p oils --lib` (`wait -n` answered 127 where 3 was due,
i.e. the operand-less `wait` had *not* spared the job) and passed when re-run
alone. Same shape as this entry, but with a cause the test owned: it backgrounded
`( exit 3 ) &` and then slept a constant `0.2` to make the job finish first, and
no constant is long enough to promise that on a loaded machine. Fixed properly
rather than by lengthening the sleep — a new `settle_jobs` test helper (the
whole-table form of the existing `settle_job`) polls `poll_jobs` until every job
has a status, after the same `JOB_EXIT_NOTICE_GRACE`. That removes this test from
the flaky family; the *corpus* case above is untouched and stays open.

---

### TD-OILS-AN-ARITHMETIC-SCAN-REPORTS-NONE-OF-THE-READS-IT-MAKES. `$(( … ))` swallows the diagnostics its nested `$( … )` should raise, and loses the text after a read that stopped early — 2026-08-14

**Where:** `userspace/oils/src/interp.rs` — `Shell::arith_extent_expand` /
`arith_extent_frame` and the `$((` route out of `Shell::arith_extent_route`.

**What is wrong.** `param_expand` reaches a `$((` through
`extract_command_subst` with `SX_COMMAND` (subst.c:10575), so the paren count
*does* recurse into a nested `$( … )` — a real parse, reported where it is met.
osh runs the count but never reports, and in one shape stops in the wrong place.
Measured against bash 5.2.37 (`build/pgX.sh` rows a/c, `build/pgY.sh` d4/d5):

| word (inside `v='…'`, via `"${v@P}"`) | bash | osh |
|---|---|---|
| `A$((1+$(echo hi⏎q` | reports EOF, `[A]` | reports EOF, `[Ahi]` |
| `A$((1+$(for⏎q))B` | reports **twice** (`for`, then `` `(1+$(for' ``), `[AB]` | silent, `[A]` |
| `A$((1+$(for⏎xB` | reports `for`, `[A]` | reports `for`, **runs `fo`**, `[A⏎xB]` |

Rows 1 and 3 report because the read runs from `Shell::arith_nested_read`,
which does call `Shell::comsub_reparse_read`; what those two get wrong is the
*value*, both by performing the abandoned extent the way the string level does
and the brace level does not. Row 2 is the substantive one: the read stopped
part way, so bash's count resumed after the `for`'s line and found the `))`,
leaving `B` to the word. osh consumes to the end and loses it — and so never
reaches the read at all, which is why it is the one row that is also silent.

**What the proper fix looks like.** The `$((` count needs the same two-outcome
treatment `${ … }` got on 2026-08-14: `Shell::comsub_reparse_read` for the
report (which also decides jump vs. no-jump), and
`Shell::failed_extent_split`'s resume point for where the count carries on.
`Lexer::unread_comsub_stop` already puts the lexer in the right place; what is
missing is the interp half — an `arith`-side counterpart of
`Shell::unclosed_brace_reads`.

**Impact.** Diagnostics only for two of the three rows; a wrong value for the
third. Needs `@P`/`PS4`/here-doc text to be reachable at all.

---

### TD-OILS-AN-UNDECODED-BRACE-BODY-IS-RE-LEXED-AS-A-DOUBLE-QUOTED-RUN. A `<(`/`>(` in it is never read, though the brace scan names it — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/interp.rs` — `Shell::extent_read_of_rest` and
`Shell::unclosed_brace_reads`, both of which lex their text with
`crate::parser::dquote_word_from_source` → `crate::lexer::lex_dquote_body`.

**What is wrong.** `extract_dollar_brace_string` names `$(`, `<(` and `>(`
together and hands each to the same `extract_command_subst` (subst.c:1881-1950),
**whatever the quoting** — that is why `x='A${z#<(fi)}B'` reports the parse
twice. A double-quoted *run*, by contrast, has no process substitution in it at
all: at string level bash and osh agree that `v='A<(echo hi⏎q'` is literal
text. So `lex_dquote_body` is the right lexer for a string-level remainder and
the wrong one for text the **brace scan** is walking.

Measured (`build/pgY.sh` d6), `A${z:-P1<(echo hi⏎S1}B` under `${…@P}`:

| | bash 5.2.37 | osh |
|---|---|---|
| reports | `` unexpected EOF while looking for matching `)' `` **then** `…: bad substitution` | the `bad substitution` only |
| value | undecoded word | same |

The `$(` spelling of the same row (`build/pgW.sh` row 5) is byte-exact, so this
is precisely the two openers `lex_dquote_body` cannot see. The dollar spelling
of d7 — where the read stops early and the brace closes — is also exact,
because that path re-lexes through `parse_braced_param_in` in
`Quoting::Unread`, which *does* read them.

**It is not only the two openers — the whole quote model is wrong** (measured
2026-08-14, `build/pq1.sh` and `build/pq2.sh`). `extract_dollar_brace_string`
**skips** a quoted run rather than walking it, and the two quotes skip
differently:

| word (inside `v='…'`, via `"${v@P}"`) | bash 5.2.37 | what it shows |
|---|---|---|
| `A${z:-P1<(echo hi⏎S1}B` | reports EOF, `bad substitution` | the bare `<(` row **is** read |
| `A${z:-P1"<(echo hi⏎S1"}B` | silent, `[AZZB]` | a `<(` inside `" … "` is **not** |
| `A${z:-P1"$(echo hi⏎S1"}B` | reports EOF, `bad substitution` | a `$(` inside `" … "` **is** |
| `A${z:-P1'$(echo hi⏎S1'}B` | silent, `[AZZB]` | a `$(` inside `' … '` is **not** |
| `A${z:-P1'<(echo hi⏎S1'}B` | silent, `[AZZB]` | …nor a `<(` |
| `A${z:-P1"<(echo hi⏎S1}B` | `bad substitution`, **no** read report | a lone `"` swallows to end of string |
| `A${z:-P1'<(echo hi⏎S1}B` | `bad substitution`, **no** read report | …and so does a lone `'` |
| `A${z:-"x"<(echo hi⏎S1}B` | reports EOF, `bad substitution` | a *closed* run does not suppress what follows |
| `A${z:-P1\<(echo hi⏎S1}B` | silent, `[AZZB]` | a backslash escapes the opener |

So the brace scan delegates a `" … "` run to a double-quote skipper that has
the `$(` row and **not** the `<(`/`>(` row — bash's ordinary rule that there is
no process substitution inside double quotes — and skips a `' … '` run whole,
offering its interior to nothing.

`lex_dquote_body` models neither — it treats both quote characters as ordinary
literals, which is correct for `Q_DOUBLE_QUOTES`, where the string *is* already
the quoted run. Measured (`build/pq1.sh`, `build/pq2.sh`, `build/pq3.sh`), osh
nevertheless agrees with bash on every *quoted* row above, by a different
mechanism in each case: where the run closes, the brace closes too and the word
goes through `parse_braced_param_in`, which does model quotes; where the run does
not close, `lex_dquote_body`'s missing `<(` row happens to suppress the same read
bash's skip suppresses. Two rows were left where the mechanisms did not coincide;
the first of them is now fixed:

| word (inside `v='…'`, via `"${v@P}"`) | bash 5.2.37 | osh |
|---|---|---|
| `A${z:-P1"$(echo hi⏎S1}B` | reports EOF, `bad substitution`, undecoded | ✅ same since 2026-08-14 |
| `A${z:-'p$(echo hi'q$(fi⏎S1}B` | reports `fi`, `[AZZB]` | silent, `[AZZB]` |

Row 1 was the serious one — a **spurious command execution**: osh reported the
EOF, then ran `S1}` and produced `[Ahi]`. A lone `"` opens a run that swallows to
end of string, leaving the brace nothing to close on, so bash condemns the word;
osh instead let the failed read out of `read_opaque_span`'s `"`-run `$(` sub-arm,
where [`Lexer::unclosed_seg`] degraded the whole word into a *string-level*
`$( … )` and then performed it. Fixed 2026-08-14 by giving that sub-arm
(`userspace/oils/src/lexer.rs`, `read_opaque_span`'s `'"'` arm) the same
`Err(e) if self.unread_comsub(&e)` recovery the two `read_dollar_brace_body`
arms already had: re-emit the `$(` into the raw text, take back what the reader
consumed with `Lexer::unread_comsub_stop`, and `continue` the quoted-run loop.
The read is still reported — it happened — and the run then swallows the rest,
so the brace never closes and the word is condemned, exactly as in bash. The bug
was **pre-existing**, not a regression: measured identical on the commit before
the earlier 2026-08-14 brace-scan fix.

Row 2 is a lost diagnostic only; the same row before the brace-scan fix had the
wrong value *and* ran `f`, so it is much improved.

**A second mechanism loses the same report where the brace *does* close**
(measured 2026-08-14, `build/pr1.sh` r3). `A${z:-'i"t'<(fi⏎S1}B` reports `fi`
in bash and expands to `[AZZB]`; osh now gets the value right (it was the
undecoded word until the unmated-`"` fix of the same day) but still says
nothing. That path never goes near `extent_read_of_rest`: the brace closed, so
the reads are replayed off the *parsed operand*, and the operand lexer is
`read_word_verbatim` in [`Verbatim::Dquote`] — which has a perfectly good `<(`
row, but never reaches it, because the `"` inside the `' … '` run opens a
quoted run that swallows `t'<(fi⏎S1` whole.

Both scans are right about their own text and wrong about each other's, which
is the shape of the whole issue: bash runs **two** passes over these bytes with
**different quote rules** — `extract_dollar_brace_string`, where a `'` run is
skipped and a `"` is a quote, and `expand_word_internal`, where a `'` is an
ordinary character and a `"` is a quote. osh derives the reads from the
expansion's lex in one path and from a string-level lex in the other, and
neither is the scan's.

**What the proper fix looks like.** A real lex entry for "text a brace scan is
walking" — not `lex_dquote_body` with a row bolted on, and not the operand lex
either. It needs, at its own level: the `<(`/`>(` openers beside `$(`; a `'`
that consumes to the next `'` or to end of string, offering nothing inside it;
and a `"` that consumes to the next `"` or to end of string, offering only `$(`
(and `` ` ``) inside it. A backslash hides the byte after it. Then
`extent_read_of_rest`, `unclosed_brace_reads` **and `brace_extent_scan`** all
take their reads from that one pass, `lex_dquote_body` keeps its current
string-level callers unchanged — the p1/p2 probe above confirms those answers
are right as they stand — and the operand lex stops being asked a question it
was never answering.

These rows are the acceptance test the table above does not already cover — the
ones that pin *which* quote wins when the two are interleaved (measured
2026-08-14 against bash 5.2.37, `build/pr1.sh`):

| word (inside `v='…'`, via `"${v@P}"`) | bash 5.2.37 | osh today |
|---|---|---|
| `A${z:-"it's"$(fi⏎S1}B` | reports `fi`, `[AZZB]` | same |
| `A${z:-"it's"<(fi⏎S1}B` | reports `fi`, `[AZZB]` | same |
| `A${z:-'i"t'<(fi⏎S1}B` | reports `fi`, `[AZZB]` | ✅ same since 2026-08-14 |
| `A${z:-P1\'<(echo hi⏎S1}B` | reports EOF, `bad substitution` | ✅ same since 2026-08-14 |
| `A${z:->(echo hi⏎S1}B` | reports EOF, `bad substitution` | ✅ same since 2026-08-14 |
| `A${z:-${y:-<(fi⏎S1}B` | reports `fi`, `bad substitution` | same |

So a `'` inside a closed `" … "` run opens nothing (rows 1-2) and a `"` inside a
closed `' … '` run opens nothing (row 3) — each quote is invisible inside the
other's run — and a backslash spends itself on the quote it precedes, leaving
the `<(` after it live (row 4).

An attempt that added only the `<(`/`>(` row to `lex_dquote_body` was written
and reverted on 2026-08-14, before being compiled, because these measurements
showed it would have regressed the three suppressed rows above (they are silent
in bash today and in osh today, and would have started reporting).

**Fixed 2026-08-14**, along the lines above. Three pieces:

- `Lexer::brace_scan` (`userspace/oils/src/lexer.rs`) — a flag saying "this
  scan stands in for `extract_dollar_brace_string`, not for the expansion after
  it". With it set, `read_double_quote_until` grows the scan's other two
  openers: a `<(`/`>(` becomes a `SubBody::Unread` segment carrying its own
  `SubDelim`, which the expansion prints straight back
  (`SubDelim::is_performed` is false for both), so the word's **value** is
  untouched and only the extent walk gains a construct to read. The new entry
  `lexer::lex_brace_scan_body` → `parser::brace_scan_word_from_source` is what
  `Shell::extent_read_of_rest` now lexes its remainder with, which is the
  unclosed-brace half (rows 4-5 of the interleaving table above).
- The closed-brace half (row 3) is the same flag turned on from
  `read_word_verbatim`'s `"` arm, and **only** when that run opened inside a
  `' … '` one — `in_run && self.here_text`. That is exactly the case where the
  scan never saw a quote at all, because it stepped over the single quotes
  whole. Outside a run the `"` is the scan's own, and there `skip_double_quoted`
  reads the `$(` spelling alone, which is what the reader already did.
- The quote state itself moved out of the lexer and into the walk, as
  `ScanQuote` (`interp.rs`): two independent flags, because
  `skip_single_quoted` hunts for a `'` and `skip_double_quoted` for a `"` and
  neither knows the other character — so each quote is an ordinary byte inside
  the other's run. `Shell::brace_scanned_subs_slice` tracks both over the
  literal runs (a `\` still hides the byte after it) and suppresses the two
  process-substitution spellings inside a `" … "` while letting `$(` through;
  `brace_scanned_subs_in` no longer resets the state on entering a
  `WordPart::DoubleQuoted` whose `"` the scan never saw.

Corpus case:
`userspace/oils/tests/corpus/the-brace-scan-reads-a-process-substitution-and-the-expansion-after-it-does-not.sh`
— 14 shapes plus a here-document body, byte-identical to bash 5.2.37 including
stderr.

**Impact while it stood.** Diagnostics only — the values already agreed. A
`<(`/`>(` at brace level lost its read report. The worst shape — a lone `"`
before a `$( … )` making osh run a command bash does not, and yield the wrong
value — was fixed earlier the same day (see row 1 of the two-row table above).
Reachable only through `@P`/`PS4`/here-doc text holding a malformed `${ … }`.

**Not fixed by this, and tracked separately:** row 2 of the two-row table,
`A${z:-'p$(echo hi'q$(fi⏎S1}B`. That one is not about the openers but about
where a construct *ends*; see
`TD-OILS-A-SQUOTE-RUN-DOES-NOT-CUT-A-SUBSTITUTION-SHORT-FOR-THE-BRACE-SCAN`.

---

### TD-OILS-AN-UNMATED-DOUBLE-QUOTE-GROWS-A-MATE-WHEN-THE-WORD-IS-PRINTED-BACK — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/unparse.rs` — `part_src`'s
`WordPart::DoubleQuoted` arm, which writes a `"` on both ends unconditionally;
the run that has no closing `"` is built by `userspace/oils/src/lexer.rs`,
`Lexer::read_word_verbatim`'s `'"'` arm under `ParseOpts::tolerant`.

**Repro** (bash 5.2.37, `build/pr11.sh` t1):

```sh
z=ZZ
v='A${z:-'"'"'i"t'"'"'$(fi)}B'; printf '[%s]\n' "${v@P}"
```

| | bash 5.2.37 | osh |
|---|---|---|
| remainder quoted by the read | `` `fi)}B' `` | `` `fi)"}B' `` |
| word named by `bad substitution` | `A${z:-'i"t'$(fi)}B` | `A${z:-'i"t'$(fi)"}B` |
| value | `[A${z:-'i"t'$(fi)}B]` | same |

**What is wrong.** In text no parser read, a `"` with no mate is not an error:
`string_extract_double_quoted` is handed a *finished word* and its walk ends at
the end of the string as readily as at a quote (that is
`ParseOpts::tolerant`, and the corpus case
`a-double-quote-with-no-mate-in-an-operand-runs-to-the-end-of-the-operand.sh`
pins the expansion of it). The resulting `WordPart::DoubleQuoted` therefore
covers a run whose closing quote **was never in the source** — but the part
does not record that, and `part_src` prints the pair back. Every consumer of
`crate::unparse::word_src` then sees one byte that was not in the word.

The value is unaffected, because quote removal drops the `"` either way. What is
affected is everything derived from the *text*: `Shell::bad_sub_word` (the word
`bad substitution` names), the tail `extract_command_subst` quotes back in its
own diagnostic, and — in principle, though no divergence has been measured for
it yet — `crate::wordscan::word_fault`, which re-scans `word_src` for the
unclosed `${`/`` ` `` verdicts and could be pushed either way by a stray quote.

The single-quote analogue exists in the same shape:
`Lexer::read_single_quote` has a `None if self.opts.tolerant => return Ok(s)`
arm, and `part_src`'s `WordPart::SingleQuoted` likewise writes both `'`s. No
divergence has been measured for it, because the paths that produce an unmated
`'` do not currently reach a diagnostic that prints the word back — but the
defect is the same one and a fix should cover both.

**What the proper fix looks like.** Record the missing mate on the part rather
than guessing at print time: `Seg::Dq(Vec<Seg>)` → `Seg::Dq(Vec<Seg>, bool)`
and `WordPart::DoubleQuoted(Vec<WordPart>)` → a `closed` field, exactly as
`Seg::Sq(Str, bool)` already carries its own flag, with `part_src` writing the
trailing quote only when it was there. About 27 sites mention `DoubleQuoted`
across `ast.rs`, `parser.rs`, `interp.rs` and `unparse.rs`; most are matches
that need only a `..`. The single-quote half is the same edit on
`WordPart::SingleQuoted`.

Not worth reaching for a cheaper trick: an unmated run always extends to the
end of its text, so "omit the quote when the part is last" would be *nearly*
right, and nearly-right quoting is how a word stops re-parsing.

**Fixed 2026-08-14**, along the lines above. `read_double_quote_until` now
reports whether a `"` really ended the run — it has exactly two `Ok` returns,
one per case, so the flag falls straight out of the existing control flow — and
that rides on `Seg::Dq(Vec<Seg>, bool)` into
`WordPart::DoubleQuoted { parts, closed }`. `part_src` writes the trailing quote
only when `closed`. The single-quote half is the same edit:
`read_single_quote`'s tolerant arm answers `false`, `Seg::Sq` became a struct
variant `{ text, escaped, closed }` rather than grow a second unnamed `bool`,
and an unmated run prints as `'` + text instead of going through
`sh_single_quote`, whose whole job is to supply the mate.

Two returns needed thought rather than transcription: the pair inside
`read_double_quote_until` that end the run on an *unclosed construct* absorbed
into a `Seg::Unclosed` answer `false`, since the run ended on the construct and
not on a quote; and the backslash spelling of `Seg::Sq` is unconditionally
`closed: true`, having no quotes to match.

Corpus case:
`userspace/oils/tests/corpus/a-double-quote-with-no-mate-does-not-grow-one-when-the-word-is-printed-back.sh`
— 8 shapes including `PS4` and a here-document body, byte-identical to bash
5.2.37 including stderr.

**Impact while it stood.** Diagnostics only — one spurious `"` in the two lines
bash prints for a malformed `${ … }` whose operand holds a `"` opened inside a
`' … '` run. Reachable only through `@P`/`PS4`/here-doc text.

---

### TD-OILS-AN-UNMATED-SQUOTE-IN-A-SUBSCRIPT-LOSES-ITS-QUOTE-BYTES-FROM-THE-WORD-PRINTED-BACK — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `attach_subscript_reads` (`userspace/oils/src/parser.rs`), which gives
each top-level `' … '` of an arithmetic fragment its interior parse.

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
declare -A m=([k]=V)
echo "[${arr['x${m:-']}]"
```

bash names `` 'x${m:-' `` — the whole fragment, quotes included. osh named
`x${m:-` — the interior of the run alone.

**Cause, which was not the one first written here.** The first note guessed the
text came from `crate::unparse::word_src` by way of `crate::wordscan::word_fault`.
It does not: `word_fault` returns `None` for these words, and the word source osh
builds is byte-correct. The diagnostic comes from `Shell::expand_unclosed` on an
`Unclosed::BadSubst` whose `text` the *interior's own lexer* filled in with
`Lexer::whole_text` — the interior being a string of osh's making. bash has no
such string: an arithmetic fragment is expanded with `Q_DOUBLE_QUOTES` set, which
switches the single quote off, so `expand_word_internal` walks straight through
the pair and the string it was handed is the fragment. Both "no closing"
reporters echo that string (`report_error (…, string)`, subst.c:1498 for
`$[ … ]`, subst.c:1972 for `${ … }`).

That also explains the shape the note found puzzling — a name that begins one
byte late and ends two bytes early is exactly the interior of a `' … '` run.
There were not two faults there, but there is a second one beside it; see
`TD-OILS-A-BRACE-WHOSE-NAME-SCAN-RUNS-OFF-A-FRAGMENT-TAKES-THE-OTHER-DIAGNOSTIC`.

**Fix.** `attach_subscript_reads` already re-measures the fragment after parsing
an interior — that is what `crate::unparse::attach_comsub_tails` does for a
`$( … )`'s echoed remainder. It now also re-*names*: a new
`name_unclosed_after_the_fragment` walks the interiors it just attached and gives
every top-level `WordPart::Unclosed(Unclosed::BadSubst { text, .. })` the
fragment's source for its `text`. Only the run's own level is renamed; a `" … "`
inside the interior is carved out by `string_extract_double_quoted` as its own
string and keeps naming itself, as one written a character to the left of the `'`
would. `src` is left alone — it is the construct's spelling for a re-print, not a
diagnostic's `%s`.

Corpus:
`a-construct-left-open-in-a-quoted-subscript-names-the-fragment-around-it.sh`
(seven rows: a `${ … }` body scan running off, the same with text after the run,
a `$[ … ]`, both substring bounds, and a run that closes nothing early).

---

### TD-OILS-A-BRACE-WHOSE-NAME-SCAN-RUNS-OFF-A-FRAGMENT-TAKES-THE-OTHER-DIAGNOSTIC — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `Shell::expand_unclosed` (`userspace/oils/src/interp.rs`) and the
`Unclosed::BadSubst` the lexer raises for it (`userspace/oils/src/lexer.rs`).

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
declare -A m=([k]=V)
echo "[${arr['x${m']}]"
```

| | |
|---|---|
| bash | `` 'x${m': bad substitution `` |
| osh | ``bad substitution: no closing `}' in 'x${m'`` |

The same string is named — that much was fixed the same day — but it is the
wrong one of bash's two messages.

**Why bash has two.** A `${ … }` in a string is read in two steps, and only the
second one is `extract_dollar_brace_string`. First `parameter_brace_expand`
extracts the *name* with `string_extract (string, &t_index, "#%^,~:-=?+/@}",
SX_VARNAME)` (subst.c:9550), which stops at one of those operator characters or
at the end of the string — `SX_VARNAME` stepping over a whole `[ … ]` subscript
on the way. If it stopped at the end, `c` is `NUL` and the `switch (c)` falls to
`default: case '\0': bad_substitution:` (subst.c:10018-10024), which is
`report_error (_("%s: bad substitution"), string)` and no longjmp. Only if it
stopped at an *operator* does the body go to `extract_dollar_brace_string`, whose
own running-out is the "no closing" one that longjmps (subst.c:1972).

So the two messages divide on whether the unclosed brace got as far as an
operator, and the division is visible:

| fragment | bash |
|---|---|
| `'x${m'` | `` 'x${m': bad substitution `` |
| `'x${#m'` | `` 'x${#m': bad substitution `` |
| `'x${m[0]'` | `` 'x${m[0]': bad substitution `` |
| `'x${m['` | `` 'x${m[': bad substitution `` |
| `'x${m:-'` | ``no closing `}' in 'x${m:-'`` |

**Two things the entry got wrong, found while fixing it.**

*It is not only a fragment.* A here-document body takes the same two messages,
and osh had the same one answer for both — `cat <<E`/`a${m b`/`E` is
`a${m b⏎: bad substitution` in bash. The `${x@P}` case really does collapse
(`no_longjmp_on_fatal_error` makes `extract_dollar_brace_string` return `NULL`
quietly and its caller fall to the same label), which is why the divergence
looked narrower than it was.

*The name scan is not the whole story.* Two checks between it and
`extract_dollar_brace_string` also reach `bad_substitution:` with an operator
already found — `valid_brace_expansion_word` on the name (subst.c:9803) and the
length branch's `string[sindex-1] != RBRACE` (subst.c:9687). So `'x${m[a:b'`
(the `:` *is* reached, but `m[a` is no name) and `'x${#q:-'` are both plain bad
substitutions. A third check, `parameter_brace_expand_indir` (subst.c:9807),
runs there too and reports in the missing brace's place: `a${!nosuch:-b` is
`nosuch: invalid indirect expansion`, and a pointer holding `not a name` is
`not a name: invalid variable name`.

**The fix.** None of this needed new state on `Unclosed::BadSubst`. osh already
had the whole decision procedure — `Shell::unterminated_brace_kind`, written for
the arithmetic-string scanner, which answers `BadSub` / `NoClosing` /
`Indir(name)` from the body text alone and has `Shell::arith_indir_resolves`
beside it for the third. `Shell::expand_unclosed` now asks it, for `close ==
'}'`, before anything else it does, and a new `Shell::unclosed_bad_substitution`
reports the `BadSub` answer naming `text` (bash's `string`) with the
`ErrexitOrPosix` class the `bad_substitution:` label carries.

Asking it *first* matters, and is bash's own order: a `$( … )` written inside
the name is walked over by `string_extract` without being parsed, so
`a${m$(fi) b` names the bad substitution and never mentions the `fi` — where osh
used to run `Shell::unclosed_brace_reads` first and report the `fi`.

**Fixed by:** `Shell::expand_unclosed` + `Shell::unclosed_bad_substitution`
(`userspace/oils/src/interp.rs`). Corpus:
`a-brace-whose-name-scan-runs-off-the-text-is-a-bad-substitution-not-a-missing-brace.sh`
— sixteen shapes covering the fragment, the here-document, the command
substitution in each half, all three indirection outcomes and the prompt
collapse, byte-identical to bash 5.2.37 including stderr.

---

### TD-OILS-AN-UNCLOSED-ARITH-SUBSTITUTION-IN-A-QUOTED-SUBSCRIPT-IS-NOT-CAUGHT-BEFORE-EXPANSION — 2026-08-14

**Where:** `crate::wordscan` (`userspace/oils/src/wordscan.rs`), the word-extent
pass `Shell::begin_word` runs before a word is expanded.

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
echo "$(touch RAN)[${arr['x$(( 1+ ']}]"
```

bash prints ``bad substitution: no closing `)' in "$(touch RAN)[${arr['x$(( 1+ ']}]"``
— the **whole word** — and `RAN` is never created. osh prints
``bad substitution: no closing `)' in 'x$(( 1+ '`` — the fragment — and the
`touch` runs.

The side effect is the real defect; the name follows from it. bash reaches this
one on the *extent* pass, before any part of the word expands, so it names the
string that pass was walking. osh reaches it only when the subscript is expanded,
by which time the substitution ahead of it has already run.

**Not the same as the two entries above.** Those are about which string a fault
found *during* the fragment's expansion names. This one is about a fault bash
finds before expansion starts and osh does not find at all until later.

**What the proper fix looks like.** `wordscan::scan` has rows for `${`,
`` ` ``, `$(`, `$[` and `<(`/`>(`, and its faults are `WordFault::Brace` and
`WordFault::Backquote`. An unclosed `$((` inside a `' … '` in a subscript is a
third: `extract_delimited_string`'s (subst.c:1498), which names the scanned
string and closes it with `)`. Adding it means teaching `word_fault` a fault that
carries its own closing delimiter, and teaching the subscript skip that a `'` in
there does not hide a `$((` from the enclosing scan.

**Impact.** A command substitution written before such a subscript runs when bash
would not have run it. Narrow, but it is a side effect and not just text.

---

### TD-OILS-A-BACKQUOTE-IN-A-QUOTED-SUBSCRIPT-IS-A-PARSE-ERROR-WHERE-BASH-EXPANDS — 2026-08-14 — ✅ FIXED 2026-08-14 (in-scope half; see the scope note at the end)

**Where:** the `' … '` interior parse of an arithmetic fragment —
`attach_subscript_reads` (`userspace/oils/src/parser.rs`) and the lexer path
behind it.

**Repro** (bash 5.2.37):

```sh
declare -a arr=(10 20 30)
echo "[${arr['x`fi']}]"
echo TAIL
```

| | |
|---|---|
| bash | ``bad substitution: no closing "`" in `fi'`` at line 2, then `TAIL` |
| osh, before | ``unexpected EOF while looking for matching `` ` ``'`` at line 4 — the script never runs |
| osh, now | identical to bash |

osh turned a runtime diagnostic into a *parse* error, so the whole script was
rejected. bash's parser stops at the `'` and resumes at its mate, so the
backquote inside is text as far as any parse is concerned; it is met only by
`param_expand`'s own `string_extract (…, SX_REQMATCH)` at expansion time
(subst.c:11269), which names `string + t_index` — the text from the backquote on.

**The fix.** Three parts:

- `Lexer::read_word_verbatim`'s `` ` `` arm used a bare `?`, which let the
  `LexError` escape as a parse error. It now converts to an `Unclosed::Backquote`
  segment via `unclosed_seg`, exactly as the `$` arm does for an unmatched `${`.
  This is the part that stopped the script being rejected.
- `Unclosed::Backquote` gained a `text` field, because its `%s` is
  `string + t_index` and not `string`: the report runs from the backquote to the
  end of the **fragment**, whereas `src` is also what `part_src`/`parts_src`
  re-print and so cannot be widened in place.
- `name_unclosed_after_the_fragment` (`parser.rs`) widens that `text` with the
  fragment tail past the run's interior — the run's own closing quote and
  whatever follows it — mirroring what it already did for `BadSubst`.

**Verified.** `userspace/oils/tests/corpus/an-unmated-backquote-in-a-quoted-subscript-is-met-at-expansion-and-not-by-a-parse.sh`
is byte-identical to bash 5.2.37, as are probes `build/pr28.sh` and
`build/pr29.sh`. Full sweep green.

**SCOPE: one residue is out of frozen scope (§305) and is deliberately left
unfixed.** Where the unmated backquote sits inside a *nested double quote* within
the run — `build/pr30.sh` d2, `echo "[${arr['x"`fi"']}]"` — bash reports
``no closing "`" in `fi"'`` and osh reports ``no closing "`" in `fi"``: osh is one
trailing `'` short. Everything else matches, including the script surviving, the
exit status and all other output. The cause is known:
`name_unclosed_after_the_fragment` visits only the run's own top level and does
not descend into a nested `DoubleQuoted` part (`crate::unparse::nested_parts_mut`
would give the descent; note its `SingleQuoted { .. }` arm returns `Vec::new()`,
so it can only supplement the outer loop, not replace it).

This is **the exact substring an error message echoes**, which design-decisions
§305 names as out of scope: nothing SlateOS runs will ever depend on it. Fix it only
if it turns up as part of something that does. The in-scope half of this
entry — a whole script being rejected where bash runs it — is closed.

**Fixed by:** the corpus case named above, plus `lexer.rs` (`Unclosed::Backquote`
`text` field, `read_word_verbatim`'s `` ` `` arm), `interp.rs`
(`Unclosed::Backquote` report) and `parser.rs`
(`name_unclosed_after_the_fragment`).

### TD-OILS-A-SQUOTE-RUN-DOES-NOT-CUT-A-SUBSTITUTION-SHORT-FOR-THE-BRACE-SCAN. A `$( … )` opened inside one swallows the read that should have followed it — 2026-08-14

**Where:** `userspace/oils/src/lexer.rs` — `Lexer::read_word_verbatim`'s `$`
arm in [`Verbatim::Dquote`], reached through
`Shell::brace_extent_scan` → `Shell::brace_scanned_subs`.

**Repro** (bash 5.2.37, `build/pr12.sh`):

```sh
z=ZZ
v='A${z:-'"'"'p$(echo hi'"'"'q$(fi
S1}B'; printf '[%s]\n' "${v@P}"
```

| | bash 5.2.37 | osh |
|---|---|---|
| reports | ``syntax error near unexpected token `fi' `` | **nothing** |
| value | `[AZZB]` | same |

**What is wrong.** The two passes bash makes over this word carve it into
*different constructs*, not merely read the same constructs differently.

- `extract_dollar_brace_string` meets the `'` and hands the run to
  `skip_single_quoted`, which stops at the **mate**. So `'p$(echo hi'` is one
  skipped run, the `$(` inside it is never seen at all, and the scan resumes at
  `q` — where it meets `$(fi⏎S1}B`, reads it, and reports `fi`.
- `expand_word_internal` has no `'` left to speak of, so its
  `string_extract_double_quoted` meets the **first** `$(`, hands the rest of the
  word to `extract_command_subst`, and — there being no `)` anywhere — takes
  everything. One substitution, not two.

osh derives the brace scan's reads from the expansion's lex, so it gets the
second carving and the second `$(` is inside the first's body, where the walk
never reaches it. `Shell::brace_scanned_subs_slice`'s single-quote bookkeeping
then correctly suppresses the one construct it *can* see (it is inside the run),
and the result is silence.

This is the residue of
`TD-OILS-AN-UNDECODED-BRACE-BODY-IS-RE-LEXED-AS-A-DOUBLE-QUOTED-RUN`, which
fixed the part of the same disagreement that was only about *which openers*
count. Rows where the two passes agree on the extents but not on the openers are
now handled by `Lexer::brace_scan`; this row is one where they disagree on the
extents, and no flag on the expansion's lex can express it.

**What the proper fix looks like.** `Shell::brace_extent_scan` has to run over
the brace's **text**, with the scan's own carve, rather than over the parsed
part. Concretely: keep the undecoded source of an unread `${ … }` on the part
(or reach it through `crate::unparse`), and lex it once in
`Lexer::brace_scan` mode with the single-quote rule the scan really has — a `'`
consumes to its mate and offers nothing inside, so a `$(` in there can neither
be read nor run past the mate. `read_word_verbatim` already computes that mate
(`sq_close`); what it does not do is let it bound a substitution, because for
the *expansion* it must not.

Note that `Lexer::brace_scan` as it stands is deliberately the narrow version:
it adds openers and leaves extents alone. Widening it to bound a `$( … )` at
`sq_close` would be wrong for the same lexer's expansion duty, so the widening
has to come with the second pass, not instead of it.

**Impact.** Diagnostics only — the value is already right. Reachable only
through `@P`/`PS4`/here-doc text holding a `${ … }` whose operand has both an
unterminated `$( … )` inside a `' … '` run and a failing one after it.

---

### TD-OILS-A-DOLLAR-BRACKET-BOUND-DOES-NOT-PERFORM-ITS-COMMAND-SUBSTITUTION. `$[ 1+$(… ]` reads the `$( … )` as an arithmetic operand token — 2026-08-14

**Where:** `userspace/oils/src/interp.rs` — the evaluation of a
`WordPart::ArithSub { bracket: true, … }` whose expression text holds an
unclosed `$( … )`.

**What is wrong.** `extract_arithmetic_subst` is
`extract_delimited_string (string, sindex, "$[", "[", "]", 0)` (subst.c:1299) —
flags `0`, so **no** `SX_COMMAND` and no nested read. The `$[` therefore closes
at its `]` by plain delimiter counting, and the unclosed `$( … )` inside is met
later, by the *arithmetic expansion* of the bounds text, which performs it under
`Q_DOUBLE_QUOTES|Q_ARITH`: it reports, runs the abandoned extent, and yields
nothing. osh instead hands the raw characters to its arithmetic tokenizer, which
calls them a bad operand.

Measured (`build/pgY.sh` d1/d2):

| word (inside `v='…'`, via `"${v@P}"`) | bash | osh |
|---|---|---|
| `A$[1+$(for⏎x]B` | reports `for`, runs `fo`, `[A1B]` | silent, `[AA$[1+$(for⏎x]B]` |
| `A$[1+$(echo hi⏎x]B` | reports EOF, `[A1B]` | silent, `[AA$[1+$(echo hi⏎x]B]` |

Row d3 — the same body with no `]` at all — is byte-exact in both shells
(silent, undecoded text), because there the `$[` genuinely never closes.

**What the proper fix looks like.** Two things, in order. (1) `$[`'s lex must
close at its `]` by plain delimiter counting, without the nested read — which
means `Lexer::read_opaque_span` needs to know its enclosing close character, so
that the `$((` spelling (SX_COMMAND) and the `$[` one (flags `0`) can part
company. Routing that arm through `Lexer::unread_comsub_stop` was tried on
2026-08-14 and reverted: it made the `$[` bounds text match bash on d1/d2, but
it *regressed* the `$((` spelling in the corpus case
`an-unterminated-construct-in-text-no-parser-read-is-a-runtime-failure`, whose
`$((1+$(echo` row must report the read and stop rather than condemn the `$((`.
A passing case outranks a documented divergence, so that arm keeps its `?`.
(2) The arithmetic evaluator must perform a `$( … )` in its expression text
with the unread-text rule rather than tokenizing it — which is what makes both
rows' values follow.

**Impact.** Wrong value and wrong diagnostic for a deprecated spelling of
arithmetic expansion, in malformed input, reachable only through `@P`/`PS4`/
here-doc text.
---

### [A] B-SMP-FAST-CPU-INDEX-PANICS-BEFORE-APIC-INIT. `smp::fast_cpu_index()` reads the APIC before it is mapped — `debug_assert` panic in debug, wild read in release — FIXED 2026-08-14

**Where:** `kernel/src/smp.rs` — the tier-3 fallback in `fast_cpu_index()`;
`kernel/src/apic.rs:~214` — `apic_read()`'s `debug_assert!(base != 0, "APIC not
initialized")`.

**What.** `fast_cpu_index()` has three tiers: RDPID, then `rdtscp`, then an APIC
MMIO read. On a CPU where neither RDPID nor `rdtscp` is advertised — which is
exactly the boot-test configuration, `qemu64,+smep,+smap,+umip` under TCG —
every call lands in tier 3 and does `crate::apic::read_id()`. Before
`apic::init` has run, `APIC_BASE_VIRT` is still 0, so:

- **debug builds:** `debug_assert!` fires → `KERNEL PANIC: APIC not initialized`.
- **release builds:** *worse* — the assert is compiled out and `apic_read`
  dereferences `(0 + offset) as *const u32`, a wild read of low memory. Silent
  garbage, or a fault, depending on what is mapped there.

**How it surfaced.** Wiring `frame_owner` ownership tagging into the frame
allocator (TD-FRAME-OWNER-1GIB) made `current_owner()` — and therefore
`fast_cpu_index()` — run on *every* frame allocation, including the allocator's
own boot-time self-test. That self-test runs long before `apic::init`, so the
kernel panicked at `[mm] Running frame allocator self-test...`:

```
!!! KERNEL PANIC !!!
panicked at kernel\src\apic.rs:214:5:
APIC not initialized
  Task: 0 (""), priority 0, cpu 0
```

**Why it was latent.** The pre-existing tier-3 callers were all gated behind
flags that only go true well after APIC init — the frame allocator's own
per-CPU cache checks `PCPU_ENABLED` first, for instance. Nothing called
`fast_cpu_index()` early, so the landmine was never stepped on. It was a real
bug regardless: the function's contract claims tier 3 "always works", and any
future early-boot caller would have hit it, in release builds silently.

**Fix.** Added `apic::is_ready()` (`APIC_BASE_VIRT != 0`) and made tier 3 check
it, returning CPU 0 when the APIC is not yet mapped. That is not a fudge: before
`apic::init` the system is strictly uniprocessor (BSP only), so 0 is the
*correct* index, not a fallback guess. Cost is one relaxed atomic load on the
already-slowest tier; tiers 1 and 2 are untouched, so real hardware pays
nothing.

**Lesson.** A "this can't happen yet" precondition that is enforced only by the
accident of who happens to call the function is not enforced at all. When the
cheap tiers of a tiered fast path are unavailable, the "always works" fallback
is the one that runs — so it is the one that has to actually always work.

### [A] B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION. The run-over-run diff named six regressions in code that had not changed — FIXED 2026-08-14

**Where:** `scripts/bench-history.py`, `diff()` / `report()`.

**Symptom.** The first post-merge `--bench` run (commit `17dbde179`, host
`Logoplex3`, BOOT_OK, exit 0) reported:

```
  REGRESSED (>25% slower):
    firewall_check: 270ns -> 482ns (+79%)
    shm_create_close: 58556ns -> 84996ns (+45%)
    ipc_semaphore: 11676ns -> 16112ns (+38%)
    net_veth_roundtrip: 47097ns -> 60102ns (+28%)
    net_veth_send: 23240ns -> 29552ns (+27%)
    io_ring_nop: 1948ns -> 2460ns (+26%)
```

**Why it was wrong.** `git diff bf26aabdb 17dbde179` over the perf-critical
paths is **two files, +54/-8**: `kernel/src/syscall/number.rs` (doc comments)
and `kernel/src/syscall/handlers.rs` (`sys_thread_join` moving its exit value
to an out-pointer). Nothing under firewall, veth, shm, semaphore or io_uring
changed at all, so not one of the six flagged benchmarks executes a line that
differs between the two commits.

The actual distribution over all 63 benchmarks: **median +6.1%, mean +9.4%,
48 slower vs. 15 faster** — and the sorted tail is a smooth continuum,
`24.4, 24.5, 24.6, 24.9, 26.3, 27.2, 27.6`. There is no gap anywhere near the
threshold. A real regression is a few outliers standing clear of a ~0% median;
what this was is a fixed 25% line drawn through the middle of a shifted
distribution.

**Root cause.** The module docstring claims run-over-run comparison "cancels
the emulation constant". That holds across *hosts*, not across *runs on one
host*: TCG is pure emulation and therefore CPU-bound, so whatever else the
machine was doing scales the whole suite by a common factor. Shift a
distribution whose own per-benchmark wobble already reaches ~20% by a further
6% and its tail crosses 25%. The `diff()` docstring even anticipated the noise
("a 10-20% wobble carries no information") but chose the wrong remedy — a
coarser *absolute* threshold cannot subtract a *global* shift, it can only
trade false positives for false negatives.

**Fix.** Added `global_drift()`: the **median** of every benchmark's
run-over-run ratio, used to normalise each ratio before thresholding, so the
threshold applies to how a benchmark moved *relative to its peers on the same
run*. The median (not the mean) is the estimator precisely because it is
unaffected by a genuine regression in a minority of benchmarks — the signal
that must not be subtracted away. Skipped below `MIN_SAMPLES_FOR_DRIFT = 8`,
where the median means nothing and a handful of benchmarks can legitimately
all move together. The report now prints the drift itself (information in its
own right — it says the machine was busy), shouts if it exceeds 15%, and shows
both numbers per entry (`+68% vs suite, +79% raw`) so no one has to trust the
correction blindly.

Replayed against the real data, the four pure-drift entries drop out and the
report goes from six regressions to three.

**Why this mattered enough to fix immediately.** It is the same class of defect
as the bug that produced this harness in the first place
(`TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE`): a report you cannot
act on. A silent skip trains you not to notice; a comparator that cries wolf on
every run trains you to skim past the one time it is right. Six false
regressions on the *very first* run it was used in anger would have retired the
feature within a week.

**Related precedent:** `TD-BENCH-OWNER-AB-BUDGET-WAS-AN-ABSOLUTE-CYCLE-COUNT`
burned five boots on "ownership tagging costs 8500 cycles" that was also the
emulator rather than the code. Same underlying trap, one level up.

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

---

### [A] W-BENCH-THREE-BENCHMARKS-ABOVE-SUITE-DRIFT-WITH-NO-MATCHING-CODE-CHANGE (original entry). firewall_check / shm_create_close / ipc_semaphore — WATCH, needs a third data point

**Where:** benchmarks `firewall_check`, `shm_create_close`, `ipc_semaphore`;
history in `bench/history.jsonl` (host `Logoplex3`).

**What.** After the drift correction above, three benchmarks still sit clear of
the suite: `firewall_check +68%` (270→482ns), `shm_create_close +37%`
(58556→84996ns), `ipc_semaphore +30%` (11676→16112ns). As established above,
none of their source changed between `bf26aabdb` and `17dbde179`.

**Why it is a WATCH and not a bug (yet).** `bench/history.jsonl` holds exactly
**two** runs on this host, so there is no per-benchmark variance estimate — the
drift correction removes the *common* factor but says nothing about how noisy
an individual benchmark is around it. `firewall_check` at 270ns is the prime
suspect for being intrinsically noisy: it is the shortest benchmark in the
suite, and at TCG timer granularity a couple of hundred nanoseconds is very
few ticks, so its relative variance should be the largest by construction.

**How to resolve.** Take a third `--bench` run on an otherwise-idle machine and
compare. If these three land back at the suite median they were noise, and the
proper fix is to give the comparator a per-benchmark variance estimate (flag on
deviation from a benchmark's own historical spread, not a flat percentage)
rather than to keep hand-adjudicating. If they stay high, they are real, and
the next question is whether the `handlers.rs` change shifted code layout
(icache/alignment) — cheap to test by benchmarking `bf26aabdb` again.

**Do not** act on either theory from the current two runs; that is exactly the
inference-from-insufficient-samples mistake the entry above documents.

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

### [A] B-BENCH-WATCHLIST-WATCHED-LESS-THAN-HALF-THE-SUITE-IT-GUARDS. `BENCH_CRITICAL_PATHS` omitted idt.rs, fs/, net/ and crypto.rs — FIXED 2026-08-14

**Where:** `scripts/boot-test.sh`, `BENCH_CRITICAL_PATHS` (feeds
`report_bench_absence`).

**What.** The list added earlier the same day to close
`TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN-BY-THE-BOOT-GATE` held five entries —
`kernel/src/{mm,sched,ipc,syscall,smp.rs}` — because it was derived from
CLAUDE.md's perf-critical *table*, read as directory names. The suite it is
supposed to guard measures far more than that. Against the 63 recorded
benchmark names:

- `isr_latency`, `page_fault` → **`kernel/src/idt.rs`**. CLAUDE.md's table
  names both "interrupt dispatch" and "page fault handling", but the handlers
  live in `idt.rs`, not under `mm/` — so the two benchmarks that measure them
  were unwatched.
- 8 × `vfs_*` (`read_256`, `write_256`, `readdir`, `stat_{root,3comp,deep}`,
  `throughput_16k_{read,write}`) → **`kernel/src/fs`**. CLAUDE.md lists "VFS
  path lookup" and "filesystem read/write" as critical.
- ~20 × `net_*`, `tcp_checksum_*`, `dns_build_query`, `firewall_check`,
  `http_*`, `dashboard_api_*` → **`kernel/src/net`** (`http.rs`,
  `dashboard.rs` live under it).
- 9 × `crypto_*` → **`kernel/src/crypto.rs`**.

So **30+ of 63 benchmarks measured code the watch list did not watch**, and a
change to any of them printed "No perf-critical changes since the last
benchmarked commit, so skipping the suite is reasonable here." Confidently,
and wrongly.

**How it surfaced.** The `W-KERNEL-COW-WRITE` diagnostic commit edits
`kernel/src/idt.rs`. The following boot reported no perf-critical changes —
while the suite contains `isr_latency` and `page_fault`, both measured by code
in that exact file. (No real regression: that diagnostic sits on the fatal
path, which is not hot. The harness had no way to know that, and did not
reason about it — it simply never looked.)

**Fix.** Widened the list to the four missing paths and annotated **every**
entry with the benchmarks it guards, so the mapping is auditable instead of
implicit. Verified: `git diff --name-only 17dbde179 HEAD` over the new list
now returns `kernel/src/idt.rs`, which the old list missed.

**Lesson (the recurring one this week).** This is the third instance in a row
of the same shape: `TD-BENCHMARKS-...` (the suite silently never ran),
`B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION` (the diff
confidently named innocent benchmarks), and now a watch list that confidently
reported "nothing to see" about a file it had never been told to look at. A
check that cannot fire is indistinguishable from a check that passes — and
every one of these was *my own* freshly-written tooling, reporting success.
When adding a guard, the first test should be "does it fire on a case I know
is positive?", not "does it run cleanly?".

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

### [A] B-BENCH-ENTIRE-SUITE-MEASURES-AN-UNOPTIMISED-KERNEL. Every recorded benchmark ran at `opt-level = 0` and was scored against optimised-reference targets — **FIXED 2026-08-14, confirmed by measurement**

> **Resolution.** `scripts/boot-test.sh` now builds `--release` and stages from
> `target/x86_64-unknown-none/release/kernel` when `--bench` is passed, and
> `bench-history.py` records/compares a `profile` field so release and debug
> records are never diffed against each other. Confirmed end-to-end by run
> `fcd066231`: the release kernel built clean (0 warnings, 9m25s), booted, and
> **`over_target` fell from 58–59 of 63 on every debug record to 15 of 63** —
> scorecard `48/63 within hardware target`. The comparator correctly refused
> to diff against the six debug records. Quantified per-benchmark evidence is
> in the RESULT section of
> `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x` above (e.g. `tcp_checksum_v4`
> 20667 → 1716 ns, `v6` 35048 → 1602 ns).
>
> Two things this did **not** settle, both tracked elsewhere and neither a
> reason to keep this entry open: (a) whether the *non-bench* boot test should
> also build release — that is **Q46**, still with the operator, and the
> default deliberately stays debug meanwhile; (b) the 15 benchmarks still over
> target, which are now genuine findings rather than codegen artefacts and
> need triage on their own merits.

**Where:** `scripts/boot-test.sh:602` (`"$CARGO" build`) and `:218`
(`KERNEL_BIN=".../target/x86_64-unknown-none/debug/kernel"`); `Cargo.toml`
`[profile.dev]` (357–365) vs `[profile.release.package.kernel]` (370–373).

**What.** The boot test builds with a bare `cargo build` — no `--release` — and
stages the artefact out of `target/x86_64-unknown-none/**debug**/kernel`. The
benchmark suite is compiled into the kernel unconditionally; `--bench` only
changes which serial marker the script waits for (`BENCH_OK` instead of
`BOOT_OK`), it does not change the build. `[profile.dev]` sets only
`panic = "abort"`, and there is no `[profile.dev.package.kernel]`, so the
kernel is built at **`opt-level = 0`**.

So every number in `bench/history.jsonl` — all 5 records, all 63 benchmarks —
measures unoptimised codegen, and every one of them is scored against
`baselines.toml` targets taken from *optimised* Linux / Fuchsia / L4 / jemalloc
implementations.

**Evidence.** Disassembling the staged binary shows textbook `opt-level = 0`
output. `tcp_checksum_bench` reloads and re-spills the accumulator to the stack
around every single add:

```
805d7181:  addl  %ecx, %eax
805d7183:  movl  %eax, -0x64(%rbp)
805d7186:  movl  -0x64(%rbp), %eax     # reload of the value just stored
```

That is one store + one load per accumulation in a loop whose entire body is
one accumulation. (`llvm-objdump` ships with the rustup toolchain — see the
path in `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x` — so this needs no binutils
install.)

**The irony.** `[profile.release.package.kernel]` already exists and is
deliberately tuned for exactly this — `opt-level = 3`, `codegen-units = 1`,
`strip = "none"` — with a comment explaining the per-package override. The
benchmark path has simply never used it.

**Why it matters.** This invalidates the *absolute* verdicts wholesale, and
they are the ones CLAUDE.md's benchmarking protocol is built on:

- The `over_target` list is not a list of subsystems that are too slow. It is
  mostly a list of subsystems compiled without optimisation. `tcp_checksum_v4`
  at 20000 ns against a 2000 ns target is a 10x miss that says nothing about
  the shipped code.
- "If a change regresses a benchmark by more than 10%, investigate before
  merging" cannot be applied to numbers whose baseline is debug codegen.
- The scale is wrong in the direction that matters: `opt-level = 0` → `3` on
  byte-loop code of this shape is routinely 5–20x. That dwarfs both the ~1.7x
  swing in `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x` and the 25% canary
  tolerance, which means the noise work done so far has been tuning the
  measurement of the wrong binary.

*Relative* commit-to-commit comparisons are not destroyed — both sides are
debug — but they are still measuring optimisation-sensitive code paths whose
debug/release ratio is not uniform, so a debug-visible change need not be a
release-visible one.

**Same family as the three before it.** `TD-BENCHMARKS-ARE-NEVER-ACTUALLY-RUN`
(the suite never ran), `B-BENCH-WATCHLIST-...` (the watch list never looked),
`B-BENCH-COMPARATOR-CALLS-SUITE-WIDE-HOST-NOISE-A-REGRESSION` (the diff named
innocents) — and now a suite that ran, reported, and was compared against
targets, while measuring a binary nobody intends to ship. A check that measures
the wrong thing is indistinguishable from a check that passes.

**Proposed fix.** Build the kernel `--release` for `--bench` runs, staging from
`target/x86_64-unknown-none/release/kernel`, and add an append-only `profile`
field to each `bench/history.jsonl` record so the comparator only ever compares
like with like. The 5 existing records must keep their meaning: absent
`profile` reads as `"debug"`, and a release record must never be diffed against
a debug one. This is a real cost — a second full kernel build, and a bench
history that restarts from zero same-profile records, which also resets the
≥6-record threshold that `TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE`
is waiting on. It is still the only honest option: a benchmark that does not
measure the shipped build is not a benchmark.

**Open sub-question:** whether the *non*-bench boot test should stay debug.
Keeping it debug preserves fast iteration and readable panics, at the cost of
two kernel builds in the tree and the risk that release-only miscompiles or
UB-dependent behaviour are only ever exercised on bench runs. Leaning toward
keeping the default boot test debug and making release the `--bench` path, but
this is worth the operator's view — see `open-questions.md`.

### [A] AUDIT 2026-08-14 — the softirq × hard-IRQ shared-lock class is clean. No action needed; recorded so it is not re-audited

**Why it was worth checking.** `softirq::process_pending` re-enables interrupts
(`kernel/src/softirq.rs`, module docs 51–56), so any lock held by a softirq
handler can be observed by a hard-IRQ handler that preempts it. That is
structurally the same failure mode as the rtl8139 deadlock and as
`B-COMPLETION-TIMER-IRQ-DEADLOCK`: the hard IRQ spins on a lock whose holder
cannot run until the IRQ returns. The intersection was believed empty only
because rtl8139 was the tree's single hard-IRQ lock acquisition — "empty by
accident" is not a property that stays true, so it needed enumerating rather
than assuming.

**What was audited.** Every callee reachable from the three softirq handlers
(`handle_timer` 355, `handle_sched` 434, `handle_irq_poll` 445):

| Callee | Lock discipline | Verdict |
|---|---|---|
| `sched::process_sleep_wakeups` (sched/mod.rs 5248) | atomic scan of `SLEEP_QUEUE`, no lock | clean |
| `sched::process_deferred_wakes` (sched/mod.rs 4897) | non-blocking wake path | clean |
| `ipc::timer::process_timer_expirations` (ipc/timer.rs 211) | explicitly non-blocking on `CP_TABLE`/`SCHED`, leaves the timer un-advanced on contention so the next tick retries | clean, and documented against `B-COMPLETION-TIMER-IRQ-DEADLOCK` |
| `ktimer::process_expirations` (ktimer.rs 323) | atomic scan of `TIMERS` | clean |
| `fs::cache::try_flush_expired` (fs/cache.rs 906) | `try_lock`, result deliberately discarded — retries in ~5 s | clean |
| `watchdog`, `kstat`, `loadavg`, `irq_storm`, `irqbalance`, `cpufreq`, `thermal` | zero `.lock()` calls; atomics only | clean |
| `rcu::tick` → `process_callbacks` (rcu.rs 483) | all three `CALLBACKS.lock()` sites (403, 486, 547) wrapped in `cpu::without_interrupts`, popping one callback per critical section and invoking it with the lock released | clean, and the comment at 393–401 records the observed 2/10 boot hang that motivated it |

`rcu` is the only softirq callee that takes a blocking lock at all, and it is
the one already hardened — the fix predates this audit and cites the boot hang
it was found by.

**Result: the intersection is empty, and empty by construction rather than by
luck.** Each site either uses atomics, uses `try_lock`, or masks interrupts for
the lock-hold window. No change was made.

**What would break it.** Adding a `.lock()` (not `try_lock`, not
`without_interrupts`-wrapped) to any callee of `handle_timer` — which is a wide
and growing list: it already fans out to 12 subsystems — while that same lock is
reachable from a hard-IRQ handler. The `handle_timer` fan-out is the risk
surface to re-check when a subsystem is added to it, not the whole kernel.

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

---

### B-VFS-STAT-ROOT-IS-12x-OVER-TARGET-AND-THE-DCACHE-IS-NOT-WHY — 2026-08-14 — OPEN (`kernel/src/fs/vfs.rs`, `kernel/src/ipc/namespace.rs`)

`vfs_stat_root` — `Vfs::stat("/")`, the single cheapest path operation the VFS
can perform — costs **6151 ns** on the release-profile run (`min` of 500
iterations, and *not* flagged by the dispersion check in that run, so the number
is clean). The CLAUDE.md target for a cached lookup is 200–500 ns per component.
For a zero-component path that is roughly **12–30x over**.

**The hypothesis I started with was wrong, and measurement is what killed it.**
`VfsDcache::lookup` (`kernel/src/fs/vfs.rs:1189`) is an O(n) linear scan over
`VFS_DCACHE_SIZE = 1024` slots, and CLAUDE.md explicitly forbids linear scans in
VFS path lookup. It was the obvious culprit and I was one step from rewriting it
as a hash table. Instrumenting first (`bench_vfs_stat_breakdown`, this commit)
showed:

```
vfs_stat_breakdown: dcache 25 valid entries (of 1024), +550 hits +0 misses
```

**25 live entries, filled from index 0, 100% hit rate.** A hit-scan terminates
in ~25 iterations, not 1024 — the cost of a linear scan is a function of
*occupancy*, not capacity. The scan cannot account for microseconds. The
1024-slot scan remains a latent defect (it degrades as occupancy grows, and it
is the *miss* path that walks all 1024) and is tracked as such below — but it is
**not** this bug's cause. Had I "fixed" it I would have burned a refactor and
moved the number by nothing.

**Where the time actually goes.** Splitting `Vfs::stat` at its own seam —
`resolve_follow(path)` then `stat_resolved(&path)`:

```
vfs_stat_breakdown_full:      6191 ns
vfs_stat_breakdown_resolved:  2442 ns
  => resolve_follow ~3749 ns (61%) + stat_resolved 2442 ns (39%)
```

So path *resolution* is the larger half, and both halves are individually over
target.

**Prime suspect for the 3749 ns, not yet confirmed.** `resolve_follow`
(`vfs.rs:1553`) calls `namespace::resolve_path` (`ipc/namespace.rs:721`), which
via `resolve_path_for` (`:735`) takes **`PROCESS_NS.lock()`**, then
**`PROCESS_ROOT.lock()`**, then conditionally **`PROCESS_MOUNTS.lock()`** — three
global spinlocks — and performs `path.to_path_buf()`, a heap allocation, *even
in the trivial `ROOT_NAMESPACE` pass-through case where the answer is the input
unchanged*. That is a fixed per-resolution cost paid by every single VFS
operation in the system. `validate_path`, `normalize_path` (another alloc), the
`VFS_DCACHE.lock()`, and `entry.resolved.clone()` (another alloc) are the other
candidates in that 3749 ns.

**Explicitly not yet attributed.** The above is a reading of the code, not a
measurement, and the last time I reasoned this way about a hot path
(`B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x`, prediction 2) I got the *sign*
wrong. The next step is to split `resolve_follow` the same way this commit split
`stat` — `namespace::resolve_path` vs `validate_path`+`normalize_path` vs the
dcache lock+clone — and let the numbers pick the target. Do not optimise any of
the four candidates before that split exists.

**Related, same shape, worse:** `vfs_stat_deep_2comp` = 33573 ns, ~16786 ns per
component against a 200–500 ns/component target. If the fixed per-resolution
prologue is the cause of `vfs_stat_root`, it does not explain this one — 2
components cost 5.4x one component, so there is a *per-component* cost here too.
Both need the same treatment.

#### PROSPECTIVE PREDICTION (written and committed before the stage-split run)

Same protocol as `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x`: the prediction is
committed before the measurement exists, so it can be graded rather than
rationalised. Last time this protocol caught me getting a *sign* wrong; the
point is to let it do that again.

**Primitive costs from the same release run** (`bench/history.jsonl`, commit
`040049442`) — these are the anchors, not guesses:

| primitive | measured | what it bounds |
|---|---|---|
| `heap_alloc_free_64` | 184 ns | one alloc+free pair ⇒ a single alloc ≲ 180 ns |
| `sched_pick_next` | 40 ns | takes the run-queue lock ⇒ an uncontended spinlock is *cheap*, ≲ 20 ns |
| `context_switch` | 1275 ns | nothing here should approach this |

**What each stage actually does** (from the code, and this is the weak part —
inspection is exactly what was wrong about the dcache):

* `ns_translate` = `current_task_id()` + `owner_process()` (a `THREAD_OWNERS.lock()` + `BTreeMap::get`) + `PROCESS_NS.lock()` + get + `path.to_path_buf()` (**1 alloc**, of a 1-byte path) + `PROCESS_ROOT.lock()` + get → `None`. So **3 spinlocks + 3 map lookups + 1 alloc**.
* `validate_normalize` = a byte scan of `"/"` + `normalize_path` (**1 alloc**).
* `dcache_hit` = `VFS_DCACHE.lock()` + ~25 path compares + `entry.resolved.clone()` (**1 alloc**).

**Predictions, falsifiable:**

1. `ns_translate` < 400 ns.
2. `validate_normalize` < 400 ns.
3. `dcache_hit` < 500 ns.
4. **Therefore the three stages sum to well under the 3749 ns that subtraction
   attributed to `resolve_follow` — I predict the sum is < 1500 ns.** Three
   allocations at ≤180 ns and six-ish uncontended spinlocks at ≤20 ns simply
   do not reach 3.7 µs.
5. **If (4) holds, the subtraction is what was wrong.** The specific mechanism I
   expect: `Vfs::stat` feeds `stat_resolved` the *resolved* path, while the
   isolated `vfs_stat_breakdown_resolved` benchmark feeds it the literal `"/"`.
   If `resolve_path("/")` returns something longer than `"/"`, then the
   `stat_resolved` inside `stat` is doing strictly more work than the isolated
   measurement of it, and subtraction charges that surplus to `resolve_follow`.
   **In that case the real culprit is `stat_resolved` — `resolve_mount`'s
   `VFS.lock()` + linear mount scan + `to_path_buf()` + `Arc::clone`, then
   `fs.lock().stat()` — and I will have misattributed the cost twice in a row
   on this one benchmark.**

This run therefore carries a direct measurement of `resolve_follow`
(`Vfs::resolve_path` is a public alias for it) *alongside* the subtraction, plus
a print of what `resolve_path("/")` actually returns. Prediction 5 is decided by
those two lines and needs no further argument.

**Standing caution, restated:** predictions 1–3 lean on the same
fine-grained cost reasoning that got the tcp_checksum sign wrong. Treat a hit as
weak confirmation and a miss as strong disconfirmation.

#### RESULT — 2026-08-14, release profile, commit `f9807f73a` (`build/stage-split.log`)

```
vfs_stat_breakdown: full 6423ns = resolve_follow ~3843ns + stat_resolved 2580ns
vfs_stat_breakdown: resolve_follow measured directly 3504ns (vs 3843ns by subtraction)
vfs_stat_breakdown: resolve_follow 3504ns = ns_translate 1948ns + validate_normalize 318ns + dcache_hit ~1238ns
vfs_stat_breakdown: resolve_path("/") -> "/" (1 bytes)
vfs_stat_breakdown: dcache 25 valid entries (of 1024), +1100 hits +0 misses over the run
```

| # | prediction | actual | verdict |
|---|---|---|---|
| 1 | `ns_translate` < 400 ns | **1948 ns** | **MISS, 4.9x** |
| 2 | `validate_normalize` < 400 ns | 318 ns | hit |
| 3 | `dcache_hit` < 500 ns | **~1238 ns** | **MISS, 2.5x** |
| 4 | three stages sum < 1500 ns | **3504 ns** | **MISS, 2.3x** |
| 5 | "the subtraction is what was wrong" | subtraction was **right** | **disconfirmed** |

**Prediction 5 was wrong in the way that matters most: it was an escape
hatch.** It said that if the stages came out cheap, the *subtraction* must be
the error and the real culprit would be `stat_resolved`. Both halves are
refuted outright by the two lines this run was built to print:
`resolve_path("/")` returns `"/"` unchanged (1 byte), so the different-inputs
hazard that would have made subtraction unsound never existed on this path; and
the direct measurement (3504 ns) agrees with the subtraction (3843 ns) to within
9.7%. `resolve_follow` really is ~55% of the whole stat, exactly where
subtraction put it. I did **not** misattribute the cost twice — I misattributed
it once, to the dcache, and then predicted I had misattributed it again in the
opposite direction. The second guess was as wrong as the first.

**Why 1 and 3 missed: a bad anchor, and it was bad by misreading the code.**
The prediction leaned on "`sched_pick_next` = 40 ns, and it takes the run-queue
lock, therefore an uncontended spinlock is ≲ 20 ns." That premise is simply
false about the benchmark. `bench_sched_pick_next` builds a **local**
`PriorityRoundRobin::new()` on the stack and calls `rq.pick_next()` directly —
it never touches `SCHED.lock()`. **It takes no lock at all.** So the one number
in the anchor table that was supposed to bound lock cost was measuring a
lock-free path, and the 20 ns figure was manufactured from nothing. This is the
same failure as the dcache: a claim about what the code does, asserted from
reading rather than from measuring, load-bearing for the conclusion.

**The cost model the measurement actually supports.** Solving the three stages
against their contents (3 locks + 3 map lookups + 1 alloc = 1948; 1 lock + ~25
path compares + 1 alloc = 1238; a byte scan + 1 alloc = 318) gives a consistent
fit at roughly:

| primitive | implied cost under QEMU-TCG |
|---|---|
| uncontended **global spinlock** acquire+release | **~500 ns** |
| heap alloc (small) | ~180 ns (matches `heap_alloc_free_64`) |
| one dcache path compare | ~21 ns |

A lock is ~3x an allocation here, and the whole path is **lock-dominated**: 4
global spinlocks across `resolve_follow` alone, ~2000 ns of its 3504. Every
optimisation instinct I had was aimed at allocations and at scan length, and
both are minor terms.

**But that model is derived, not measured, and deriving is what just failed
twice.** So the next run adds `bench_spinlock_uncontended` to measure the
primitive directly. The suite has anchors for allocation, context switch and
syscall dispatch but none for the single most common operation in the kernel,
which is precisely why a fabricated 20 ns figure went unchallenged.

**Consequences (tracked as `B-NAMESPACE-RESOLVE-TAKES-3-GLOBAL-LOCKS-TO-RETURN-ITS-INPUT` below).**
`ns_translate` is 1948 ns — 56% of `resolve_follow`, 30% of the entire stat —
and for a process in the root namespace with no chroot and no volume mounts
(i.e. every process on a normal desktop) it does all of that work to **return
its input unchanged**.

---

### B-NAMESPACE-RESOLVE-TAKES-3-GLOBAL-LOCKS-TO-RETURN-ITS-INPUT — 2026-08-14 (`kernel/src/ipc/namespace.rs`)

**Measured, not inferred:** `ns_translate` = **1948 ns**, which is 56% of
`resolve_follow` and **30% of an entire `stat("/")`**. See the RESULT section of
`B-VFS-STAT-ROOT-IS-12x-OVER-TARGET-AND-THE-DCACHE-IS-NOT-WHY` above.

`namespace::resolve_path` is called before **every** path operation in the VFS —
read, write, stat, open, mkdir, unlink, all of it. For a process in the root
namespace with no chroot and no volume mounts — which is every process on a
normal desktop, and every process in this kernel today — the entire function
body is:

1. `current_task_id()` — cheap, an atomic load.
2. `owner_process(task_id)` → **`THREAD_OWNERS.lock()`** + map get.
3. **`PROCESS_NS.lock()`** + map get → `ROOT_NAMESPACE`.
4. `path.to_path_buf()` — a heap allocation.
5. **`PROCESS_ROOT.lock()`** + map get → `None`.
6. Return the path, byte-for-byte identical to the input.

**Three global spinlock acquisitions and one heap allocation, to return the
argument unchanged.** At the measured ~500 ns per uncontended global spinlock
under TCG, the locks alone are ~1500 of the 1948 ns.

This is not a micro-optimisation target, it is a missing fast path. The
structure charges every path operation in the system for a feature (containers)
that is not in use, and the charge is paid in the most expensive primitive
available.

**The fix** — a global "namespace features are in use" flag, checked with one
relaxed atomic load before any lock is taken:

* An `AtomicBool` (`NS_FEATURES_ACTIVE`) set with `Release` ordering at the
  three sites that can make namespace state non-trivial: inserting into
  `PROCESS_NS`, into `PROCESS_ROOT`, and into `PROCESS_MOUNTS`.
* `resolve_path_for` loads it with `Acquire`; if clear, it returns immediately.
* **The flag is never cleared.** Clearing it on the last teardown would
  introduce a race with a resolve already in flight, and the cost of staying on
  the slow path after containers have been used once is exactly the cost we have
  today. Monotonic is the sound choice and it is deliberate, not an oversight.

This is the standard rarely-used-feature pattern (Linux's static keys). It does
not change behaviour for any process: with the flag clear, no process has a
namespace, a root, or a volume, so every branch the slow path could take is the
identity branch — which is what makes the fast path a refactor rather than a
semantic change.

**The allocation in step 4 survives this fix** and is the correct next target:
`resolve_path` returns `PathBuf`, so the pass-through allocates a copy that
`resolve_prologue` immediately re-allocates in `normalize_path`. Returning
`Cow<'_, Path>` would remove one of the two. Deferred until the lock fix is
measured, because at ~180 ns it is a third of a single lock and chasing it first
would have been another instance of optimising the minor term.

#### PROSPECTIVE PREDICTION (recorded before the fix is built)

Same protocol, and this time with a directly measured anchor rather than a
fabricated one — the next run also adds `bench_spinlock_uncontended`.

1. `bench_spinlock_uncontended` comes out in **300–700 ns**. This is the load-
   bearing one: the whole cost model above stands or falls on it. If it lands
   below ~150 ns, the lock attribution is wrong and something else in
   `ns_translate` is the real cost.
2. `ns_translate` drops from 1948 ns to **< 150 ns** (one atomic load, one
   allocation removed only if the `Cow` change lands too — so expect ~180 ns if
   the allocation stays; I predict the allocation is skipped entirely on the
   fast path, hence < 150).
3. `resolve_follow` drops from 3504 ns to **1700–2000 ns**, now dominated by
   `dcache_hit`.
4. Full `vfs_stat_root` drops from ~6151 ns to **~4400–4700 ns**, a ~28%
   improvement on a benchmark I twice tried to fix by looking at the wrong
   subsystem.

**If (1) holds but (2) does not**, the fast path is not being taken — most
likely because some process really did set one of the three maps during boot,
which would itself be worth knowing and is why the benchmark prints the flag.

#### RESULT — 2026-08-14, two post-fix release boots ✅ FIXED

The first post-fix boot reported `namespace fast path DISABLED
(NS_FEATURES_ACTIVE=true)` — the pre-registered fallback clause above, firing
verbatim. The cause was not "some process set one of the maps during boot" but
something better: **the namespace self-tests themselves**.
`test_process_attach_detach`, `test_process_root` and `test_volume_mounts` call
`attach`/`set_root`/`add_volume`, which arm the monotonic flag, and nothing
disarmed it. So the self-tests were permanently degrading the VFS of the kernel
they had just finished validating — every path operation for the rest of the
boot paid three global spinlocks to exercise a feature that no longer had a
user. Fixed by asserting `reset_ns_features_if_trivial()` at the end of
`self_test()`, which doubles as a leak check: it can only succeed if every
namespace test cleaned up its process state.

Two boots after that fix (the first aborted on an unrelated flake — see
`B-FASTPY-SLEEP-SELF-TEST-IS-FLAKY` — so both are reported):

| # | prediction | pre-fix | run A | run B | grade |
|---|---|---|---|---|---|
| 1 | uncontended tracked lock **300–700 ns** | 628 | 448 | 632 | **HIT** (all three in band) |
| 2 | `ns_translate` **< 150 ns** | 1670 | 347 | 264 | **MISS** (1.8x over) |
| 3 | `resolve_follow` **1700–2000 ns** | 3138 | 2488 | 1627 | **UNPROVEN** (band narrower than the noise) |
| 4 | `vfs_stat_root` **4400–4700 ns** | 5930 | 2971 | 4394 | **HIT** (run B lands 0.14% under the band) |

**(1) HIT, and it was the load-bearing one.** The previous prediction on this
benchmark failed because its lock cost came from a *fabricated* anchor; this one
was measured first, and everything built on it held.

**(2) MISS, and the miss was avoidable by reading a type signature.** The
prediction said "I predict the allocation is skipped entirely on the fast path,
hence < 150". It cannot be: `resolve_path` returns `PathBuf`, so *every* return
allocates, fast path or not. The residual ~264 ns is one atomic load plus that
allocation. This is not a measurement surprise — it is a claim contradicted by
the function's own declaration, which was there to be read. It also promotes the
deferred `Cow<'_, Path>` change from "the correct next target" to "the only
remaining term".

**(3) UNPROVEN, and that is the more useful result.** 1627 and 2488 straddle the
band. The two runs differ by 1.53x while the band spans 1.18x — the prediction
was finer-grained than the instrument meant to grade it. Predicting to a
precision the measurement cannot resolve yields a verdict that is noise wearing
a grade's clothes, which is worse than no verdict. See
`TD-BENCH-STAGE-SPLIT-HAS-NO-COHERENCE-CHECK` below, where the same two runs
disagree by 1.67x on two byte-identical benchmarks.

**(4) HIT.** Predicted "~28% improvement"; measured −26% (5930 → 4394). This is
the benchmark twice attacked in the wrong subsystem (first the dcache, then the
subtraction). The third attempt — measure the anchor, then follow the
measurement — worked on the first try.

---

### B-LOCKDEP-CLASS-LOOKUP-IS-A-LINEAR-SCAN-ON-EVERY-LOCK — 2026-08-14 (`kernel/src/lockdep.rs`)

**Measured, and it is the largest single overhead found this session.** The lock
microbenchmark added to grade the namespace fix answered a question nobody had
asked it:

```
lock acquire+release: raw 30ns, tracked 632ns, no-lockdep 232ns, no-stats 656ns
lock overhead: total +602ns = lockdep 400ns + preempt 29ns + rdtsc 57ns + unexplained 116ns
```

`raw` is `spin::Mutex`; `tracked` is `crate::sync::Mutex`, the type every global
in the kernel uses. **The tracked mutex costs 21x the raw one, and two thirds of
the difference is lockdep.** Confirmed across both post-fix boots: 400/602 ns
(66%) and 281/430 ns (65%).

The cause is not that validation is expensive. It is that the *lookup* is
`O(classes)`:

```rust
fn find_or_register_class(lock_addr: usize, name: &[u8]) -> Option<u16> {
    let count = CLASS_COUNT.load(Ordering::Relaxed) as usize;
    for i in 0..count.min(MAX_CLASSES) {          // <-- up to 128 iterations
        if unsafe { CLASSES[i].id } == lock_addr { return Some(i as u16); }
    }
    ...
```

and `find_class` — called from `lock_release` — is the same scan again. So every
lock operation in the kernel walks the class table **twice**, and `MAX_CLASSES`
is 128. This is exactly the "linear scan on a hot path" CLAUDE.md's performance
section forbids, hiding inside the *debugging* infrastructure rather than the
code being debugged, which is why no amount of reading the subsystem under
investigation would ever have found it.

Two further consequences worth stating because they distort the whole benchmark
suite:

* **The cost is positional.** A lock class registered early is found in a few
  iterations; one registered late pays the full scan. So the same lockdep call
  is cheap or expensive depending on *boot order*, and a benchmark's own lock —
  registered last, at benchmark time — pays the worst case. The 400 ns figure is
  therefore an upper bound on the average, not the average.
* **Every benchmark in this suite that takes a lock is partly measuring this.**
  `syscall_dispatch` (653–699 ns), `futex_wake_empty` (953 ns) and the VFS
  numbers all include it.

**The fix** (implemented in the same change as this entry): an open-addressed
hash index from lock address to class slot, Fibonacci-hashed and linearly
probed, 512 buckets for 128 classes so the load factor stays at 25%. This is
what Linux does (`classhash_table`, `kernel/locking/lockdep.c`). Entries are
append-only, so a probe run is contiguous and stopping at the first empty bucket
is correct.

**This fix is what makes the tempting question go away.** The obvious reaction to
"lockdep costs 400 ns per lock" is to gate it to debug builds, as Linux does with
`CONFIG_PROVE_LOCKING` — trading deadlock detection in production for lock speed.
That would have been a real architectural fork worth escalating. It is moot: the
validator was never inherently expensive, its index was. Keep both.

**The optimisation is guarded by a test that can actually fail.** A hash that
silently *misses* a registered class is the dangerous failure: `find_or_register_class`
would then register a second class for the same lock, that lock's dependency
edges would split across two graph nodes, no cycle would ever be found through
it, and lockdep would go quiet — looking exactly as healthy as a kernel with no
deadlocks. So the linear scan is not deleted, it is demoted to an oracle:
`test_class_hash_index()` asserts the hash and the scan agree on every registered
class, agree on absence, that double registration yields one class, and — using
a colliding address it *searches for* rather than hopes for — that the probe
sequence survives a bucket collision.

#### PROSPECTIVE PREDICTION (recorded before the fix is booted)

1. `lock_tracked` drops from ~632 ns to **250–330 ns**, i.e. close to the
   measured `no-lockdep` figure (232 ns) plus a hash lookup and probe (~2 memory
   references, call it 20–80 ns under TCG). If it lands *below* 232 ns something
   is wrong — the index cannot be cheaper than not running at all.
2. `lockdep` in the overhead split drops from ~400 ns to **< 100 ns**.
3. The knock-on: `syscall_dispatch` (653–699 ns across four boots, target 200)
   improves by **at least 15%**, because it takes tracked locks. This is the
   riskiest of the three — if syscall dispatch does *not* move, then either it
   takes no tracked lock or the lock is registered early enough to have been
   cheap already, and the "every benchmark is partly measuring lockdep" claim
   above is overstated and must be narrowed.
4. `lockdep classes registered` (newly printed) comes out **> 40**. If it is in
   single digits, the scan was never long and the 400 ns has some *other* cause
   inside `lock_acquire` — most likely `smp::current_cpu_index()` or the
   re-entrancy guard — and this whole diagnosis is wrong.

#### RESULT — 2026-08-14, release boot ✅ FIXED

```
[lockdep]   class hash: OK (3 classes verified vs scan, bucket collision handled)
[bench]   lock acquire+release: raw 25ns, tracked 274ns, no-lockdep 223ns, no-stats 301ns
[bench]   lock context: 43 lockdep classes registered
[bench]   lock overhead: total +249ns = lockdep 51ns + preempt 29ns + rdtsc 56ns + unexplained 113ns
[bench] SCORE lock_uncontended 274 500 PASS
```

| # | prediction | before | after | grade |
|---|---|---|---|---|
| 1 | `lock_tracked` **250–330 ns** | 632 | **274** | **HIT** |
| 2 | lockdep's share **< 100 ns** | 400 | **51** | **HIT** (7.8x) |
| 3 | `syscall_dispatch` improves **≥ 15%** | 653–699 | **699** | **MISS** (0%) |
| 4 | **> 40** classes registered | — | **43** | **HIT** |

**The tracked mutex went from 21x the raw spinlock to 11x, and
`lock_uncontended` moved from OVER to PASS** (274 vs the 500 ns target). Knock-on
in the same boot: `vfs_stat_root` 4394 → **3344 ns**, so with the namespace fast
path the total on that benchmark is **5930 → 3344, −44%**.

**(3) MISS, and the pre-registered consequence is honoured rather than
explained away.** The prediction said: *"if syscall dispatch does not move, then
the 'every benchmark is partly measuring lockdep' claim is overstated and must be
narrowed."* It did not move — 699 ns, identical to the best of the four pre-fix
boots. **Narrowing it: the claim was overstated.** Lockdep taxed benchmarks that
take `crate::sync::Mutex` *specifically*, which is the VFS/namespace path, not
"every benchmark that takes a lock". `syscall_dispatch` evidently takes none, or
takes a different lock type (`PreemptSpinMutex`, which is a distinct type with
distinct overhead — a distinction this session already had to write a comment
about in `bench.rs`). `syscall_dispatch` at 3.5x its 200 ns target is therefore
still unexplained and remains open.

**The coherence gates from `TD-BENCH-STAGE-SPLIT-HAS-NO-COHERENCE-CHECK` shipped
in the same boot and reported a clean run:** drift 3331 → 3353 ns (0%),
parts/whole 96%. That is the *quiet* outcome, so it proves only that the gates do
not fire spuriously — **it does not prove they fire.** They have not yet been
observed rejecting a run, and until they have, they carry exactly the weakness
this file keeps documenting. The two incoherent runs that motivated them are
recorded above, so the next drifting boot is the test.

**Weak spot in the new test, recorded rather than glossed:** `test_class_hash_index()`
runs from `lockdep::self_test()`, which executes early in boot when only **3**
classes are registered — but the pathology it guards against (a probe run
walking into a collision) needs a *populated* table, and by benchmark time there
are 43. The synthetic collision case is what carries the test today; the
verify-every-registered-class part is checking 3 of the eventual 43. It should be
re-run late in boot as well. Tracked as
`TD-LOCKDEP-HASH-TEST-RUNS-BEFORE-THE-TABLE-IS-POPULATED`.

---

### TD-LOCKDEP-HASH-TEST-RUNS-BEFORE-THE-TABLE-IS-POPULATED — 2026-08-14 — ✅ FIXED 2026-08-14 (`kernel/src/lockdep.rs`, `kernel/src/main.rs`)

`test_class_hash_index()` verifies the O(1) class index against a linear-scan
oracle, but it is called from `lockdep::self_test()` during early boot, when the
class table holds **3** entries. By the time the kernel is doing real work it
holds **43**. So the "every registered class is found at the index the scan
reports" assertion — the one that would catch a probe-sequence bug — is
exercised at 7% of the table size it needs to defend.

The synthetic part of the test (register a fresh address, then register a
deliberately colliding one and check both resolve) does not depend on table size
and is doing the real work today. That is why this is tech debt and not a hole:
the collision path *is* covered, just not at realistic occupancy.

**Proper fix:** expose it as `pub fn verify_class_index()` and call it a second
time late in boot — after driver/subsystem init, when the table is full — so the
oracle comparison runs against all 43 classes. It must run on every boot, not
only `--bench` boots, or it inherits the "check that only runs when you're
already looking" problem.

> **Resolution.** Done as described; the call takes a `when` label so the two
> runs are distinguishable in the log and the vacuous early pass cannot be
> misread as the meaningful one:
>
> ```
> [lockdep]   class hash (early): OK (3 classes verified vs scan, bucket collision handled)
> [lockdep]   class hash (populated): OK (31 classes verified vs scan, bucket collision handled)
> ```
>
> **The placement was itself the interesting part, and got it wrong on the first
> attempt.** The late call went in next to the deferred-benchmark spawn, which
> reads as "late in boot" — but that sits *after* `BOOT_OK`, and
> `boot-test.sh` kills QEMU at `BOOT_OK` unless `--bench` is given. So the first
> version printed nothing on a normal boot test: a check that would have run only
> on benchmark boots, i.e. only when someone was already looking, which is the
> precise failure mode it was added to prevent. Moved above the `BOOT_OK` marker,
> with a comment at the site saying why it must stay there. Verified by the
> absence-then-presence of the line across two boots, not by reading the code.

**Residual, not worth a separate entry:** 31 classes at `BOOT_OK` versus 43 by
benchmark time — the last dozen register during post-boot activity. Coverage is
now 72% of the eventual table rather than 7%, and the synthetic collision case
covers the probe path independently of occupancy.

---

### TD-BENCH-STAGE-SPLIT-HAS-NO-COHERENCE-CHECK — 2026-08-14 (`kernel/src/bench.rs`)

Two byte-identical benchmarks, in the same boot, disagreed by 1.67x:

```
SCORE vfs_stat_root 2971 ...
[bench] vfs_stat_breakdown_full: min=25808 cycles (4976ns) ...
```

Both are `run(..., 500, || black_box(Vfs::stat("/")))`. Nothing distinguishes
them but *when in the boot they ran*. In the next boot the same pair came out
4394 and 4306 — coherent. So the harness's min-of-500 is sometimes accurate and
sometimes 1.7x off, and **nothing in the output says which kind of run you are
reading.**

The consequences are not hypothetical; they are the two runs above:

* Run A attributed `stat_resolved` 2531 → 4109 ns, a 62% "regression" caused by
  a change that cannot touch it.
* Run B printed `full 4306ns = resolve_follow ~0ns + stat_resolved 5762ns` — the
  subtraction saturated at zero because a *part* measured larger than the
  *whole*. That is arithmetically impossible and it was printed without comment.
* Run A's parts summed to 133% of its whole. Also printed without comment.

This is the project's recurring defect class in its purest form: the check was
*there* — the code deliberately measures `resolve_follow` both directly and by
subtraction, with a comment explaining that a disagreement would indict the
subtraction — and then prints both numbers side by side and says nothing when
they disagree by 2.9x. **A check whose failure is not distinguishable from its
success is not a check, it is a decoration.**

> **Resolution (same change).** Two gates, both of which say the word WARNING:
>
> * **Drift gate.** The first measurement (`vfs_stat_breakdown_full`) is repeated
>   verbatim at the *end* of the block as `..._full2`. The two are the same code
>   over the same input, so any difference is pure measurement drift across the
>   width of the block, and it bounds how much of every stage difference is real.
>   Over 25% and the run is declared not internally coherent and unusable for
>   attribution.
> * **Parts/whole gate.** `resolve_direct + stat_resolved` must land within
>   75–125% of `full`, or the stage attribution is declared "not arithmetic, it
>   is noise".
>
> The same discipline is applied to the new lock benchmark, which prints
> `unexplained` as an explicit residual and warns when the components exceed the
> total they were subtracted from.

**Not fixed:** the harness still reports a single `min` with no confidence
interval, so a *single* benchmark with no in-block replicate (i.e. all the
others) remains ungraded for coherence. The proper fix is for `run()` itself to
take two interleaved sample sets and report their disagreement, making every
benchmark self-checking rather than just this one. Tracked here; not blocking.

---

### B-FASTPY-SLEEP-SELF-TEST-IS-FLAKY — 2026-08-14 (`kernel/src/proc/spawn.rs:15508`)

`self_test_fastpy_slateos_sleep()` failed one release boot and passed the next
with no relevant code change in between:

```
[spawn]   FAIL: fastpy-sleep (ring 3) — reached Zombie but exit code was Some(3),
          expected 0 (3 = a clock read was 0 or the observed sleep delta was < 40000000 ns)
```

The tool printed its measured delta: **36 818 000 ns for a `time.sleep(0.05)`**,
i.e. the sleep returned **26% early**, against a 40 ms lower bound. The kernel's
own `[sched] sleep_ns` test in the same boot passed (`slept 20.459ms for 20ms
request`), so whatever is short is not the scheduler's `sleep_ns` at a 20 ms
scale.

Two candidate causes, not yet separated:

1. **The sleep genuinely returns early at 50 ms** — a wakeup-deadline rounding or
   timer-phase bug that a 20 ms request happens not to expose.
2. **`clock_realtime()` advances more slowly than real time** during the sleep,
   so a correct 50 ms sleep *reads* as 36.8 ms. Ratio 50/36.818 = 1.358, which is
   suspiciously close to nothing in particular, but the two clocks the test
   compares (the scheduler's timer and `clock_realtime`) are different sources
   and their agreement is exactly what the test implicitly assumes and never
   checks.

Distinguishing them is cheap and should be done before touching anything: have
the harness log its own `clock_realtime()` delta across the child's lifetime
next to the child's measured delta. It already reads both — guard #2 in the
doc comment — but only compares each against the bound, never against each
other. If the kernel-side delta is ~50 ms while the child's is ~37 ms, it is
cause (2) and the bug is in the userspace clock path, not the sleep.

Impact today: an intermittently red boot test, which is corrosive — a suite that
cries wolf gets its failures ignored, and this is the only ring-3 test of the
blocking-sleep path. Not lane-A-exclusive (the tool is fastpy/userspace), but
the timekeeping and `SYS_SLEEP` sides are, and the harness is in
`kernel/src/proc/spawn.rs`.

#### CORRECTION 2026-08-14 — the proposed discriminator cannot discriminate

The plan above ("have the harness log its own `clock_realtime()` delta next to
the child's") **would not have separated the two causes**, because the two
numbers it compares come from *the same clock*. `SYS_CLOCK_REALTIME` returns
`timekeeping::clock_realtime()`; the harness calls
`timekeeping::clock_realtime()`. Under cause (2) — that clock running slow —
both readings compress by the same factor and the comparison shows nothing.
The test would have been "instrumented" and still blind: one more instance of
this file's recurring defect, *a check that cannot fire is indistinguishable
from a check that passes.*

The real discriminator was already in the tree, unread. Reading the call chain:

| stage | clock |
|---|---|
| `sleep_ns` computes and enforces its deadline | `hrtimer::now_ns()` → **HPET** (`kernel/src/hrtimer.rs:147`) |
| the child, and the harness, measure the elapsed time | `timekeeping::clock_realtime()` → **TSC**, via `clock_monotonic()` (`kernel/src/timekeeping.rs:154`) |

So the sleep is *enforced* against one oscillator and *measured* against
another, and the test silently assumes the two agree. That assumption is the
untested one, and it is the whole bug surface:

- If HPET and TSC agree across the window, the sleep really did return early —
  **cause (1)**, a deadline/timer-phase bug in `sleep_ns`.
- If HPET says ~50 ms while TSC says ~37 ms, the sleep was correct and the
  **TSC calibration** (`bench::tsc_freq()`) is off by that ratio — cause (2),
  and then it is not a userspace clock bug at all but a kernel calibration one,
  which would also skew every `clock_realtime()` consumer and every
  wall-clock-derived figure in the tree.

Note the observed ratio: 50 / 36.818 = **1.358**. The entry above called that
"suspiciously close to nothing in particular" — but as a *TSC calibration*
error it needs no numerological explanation; a mis-measured `tsc_freq` can land
anywhere, and under TCG the calibration loop is exactly the kind of thing a
busy host perturbs. That reading also explains the flakiness the entry opens
with: a calibration performed once per boot, on a host whose load varies, gives
a different scale factor on each boot — so the same correct sleep reads 50 ms
on a quiet boot and 37 ms on a busy one. **The intermittency is evidence for
cause (2), and the original framing had no account of it at all.**

The instrument therefore is: sample **both** `hrtimer::now_ns()` and
`timekeeping::clock_realtime()` either side of the child's lifetime, print both
deltas and their ratio, and print them on the *failure* path too — today
`kernel_elapsed` is computed at `spawn.rs:15623`, *after* the guard-#1 early
return at 15611, so on the exact runs that fail, the one number that would
explain the failure is never printed.

**Prediction P16** (registered before the measurement exists): on a boot where
the child reports < 40 ms, the HPET delta will exceed the TSC delta by >= 1.2x
— cause (2). MISS if the two agree within 5%, which puts it back on `sleep_ns`.

---

### TD-BASELINES-TOML-IS-INVALID-TOML-AND-NOTHING-READS-IT — 2026-08-14 — ✅ FIXED 2026-08-14 (`bench/baselines.toml`, `scripts/test-bench-history.py`)

`bench/baselines.toml` — the file CLAUDE.md names as the place performance
baselines live, and which ~30 comments across `kernel/src/bench.rs` cite as
their source — **did not parse as TOML.** It carried two `[compositor_frame_4k]`
tables, at lines 296 and 389, which is a hard error in every conforming parser:

```
tomllib.TOMLDecodeError: Cannot declare ('compositor_frame_4k',) twice
                         (at line 389, column 21)
```

The two disagreed about the **unit**: `target_ns = 2000000` in one,
`target_ms = 2.0` in the other. Only one carried the measured figure and the
optimisation history (48.6 ms → 21.4 → 15.8 → 11.9 → 10.6 ms). So the file had
been carrying two contradictory records of the same benchmark, and a parser
that tolerated duplicates would have silently taken whichever came last.

**Why it survived: nothing reads the file.** Every reference to it in the tree
is a *comment*. `kernel/src/bench.rs` hard-codes each target as a literal with
`// Target from baselines.toml: < 200 ns` beside it; `scripts/bench-history.py`
never opens the file. So the file *looked* like the authority while the real
authority was ~60 scattered literals in Rust, and no parser was ever pointed at
the thing that was supposed to be the source of truth.

**This is the fifth instance of the same defect class**, after
`TD-BENCHMARKS-...` (the suite never ran), `B-BENCH-WATCHLIST-...` (the watch
list never looked), `B-BENCH-COMPARATOR-...` (the diff named innocents) and
`TD-BENCH-CANARY-...` (the canary never fired). The invariant keeps holding: *a
check that cannot fire is indistinguishable from a check that passes.* Here it
went one step further — the artefact could not even be **loaded**, and that too
was indistinguishable from health, because loading was never attempted.

> **Resolution.** The duplicate table is merged (the poorer one removed, with a
> comment at the site recording why). `scripts/test-bench-history.py` gained
> `test_baselines_is_valid_toml()`, which `tomllib.load`s the real file — so the
> file is now machine-read for the first time and a duplicate or syntax error
> fails the suite. The test also asserts every table names a target in some
> unit, matched by `target*` **prefix** rather than an enumerated list (the
> units are open-ended by design: `target_accesses_over_nop` and
> `target_accesses_delta` exist because TCG harness overhead swamps the
> absolute number, and an enumerated list would silently under-report the day
> it wasn't extended). Calibration constants and host metadata opt out via a
> declarative `not_a_target = true` in the data rather than a name list in the
> test. Writing that assertion immediately found four more tables to classify.
> 16 checks pass.

**Not fixed — the duplication itself.** Targets still live in two places: this
file and the literals in `bench.rs`, with nothing keeping them in sync, so they
can drift silently and at least one (`vfs_stat_root`: 700 ns in the file) should
be re-derived anyway. The proper fix is for the kernel's scorecard to be checked
against the parsed file by `bench-history.py`, so the file becomes the authority
it already claims to be. Blocked on nothing but effort; tracked here.

#### FOLLOW-UP 2026-08-14: with the file finally parseable, the drift is measurable — and it is near-total

Making `baselines.toml` load was worth doing for its own sake, but the first
thing a working parser bought was a number for the damage. Matching the 63
benchmark names the kernel prints against the 57 baseline tables:

| | count |
|---|---|
| benchmarks measured by the kernel | 63 |
| baseline tables in the file | 57 |
| **matched by name** | **30** |
| measured with no baseline at all | 33 |
| baselines naming a benchmark never measured | 27 |

**Less than half of what runs has a baseline it can be compared to.** And the
two lists are not describing different work — they are largely the *same*
benchmarks under two names, drifted apart because nothing ever had to reconcile
them:

| kernel prints | baselines.toml calls it |
|---|---|
| `syscall_dispatch` | `syscall_trivial` |
| `page_fault` | `page_fault_anon` |
| `tcp_checksum_v4` | `net_tcp_checksum_v4_1460b` |
| `tcp_checksum_v6` | `net_tcp_checksum_v6_1460b` |
| `vfs_stat_deep` | `vfs_stat_deep_2comp` |
| `vfs_throughput_16k_read`/`_write` | `vfs_throughput_16k` |
| `heap_alloc_free_64` | `heap_alloc_small` |
| `ipc_channel` | `ipc_channel_roundtrip` |
| `ipc_pipe` | `ipc_pipe_roundtrip` |
| `ipc_eventfd` | `eventfd_signal_read` |
| `ipc_semaphore` | `semaphore_signal_wait` |
| `firewall_check` | `net_firewall_inbound_check` |
| `dns_build_query` | `net_dns_build_a_query` |
| `io_ring_nop` | `iouring_sqe_submit` |
| `isr_latency` | `interrupt_dispatch` |
| `service_connect` | `service_connect_accept` |
| `cp_notify_wait_rt` | `cp_notify_wait_roundtrip` |
| `net_tcp_conn_lookup` | `net_tcp_conn_table_scan` |

That is 18 of the 33 unmatched accounted for as pure renames. The remainder
split into benchmarks genuinely lacking a baseline (`vfs_stat_root`,
`vfs_read_256`, `vfs_write_256`, `vfs_readdir`, `vfs_stat_3comp`,
`http_gzip_*`, `ipc_channel_sync`, `net_arp_lookup`, `net_checksum`,
`net_ethernet_parse`, `net_ipv4_parse`, `pick_next`, `sched_pick_next`) and
baselines for work that is not benchmarked at all (`futex_uncontended`,
`futex_contended_wake`, `futex_wait_mismatch`, `compositor_frame_4k` — the last
is Lane C's and is measured by a host-side `cargo test`, not by this suite).

**Note what this does to the headline number.** The `over_target` count the
kernel reports (15 of 63 on the release run) is computed from the literals in
`bench.rs`, not from this file — so it is not wrong, but it is also not
*checkable* against the stated baselines for the 33 unmatched. Ranking the
release run against the parsed file yields only 7 over-target entries, and that
smaller number is an artefact of the missing half, not good news. Notably
`vfs_stat_root` — the benchmark currently under investigation at 8.5x over — has
**no** table here at all; its 700 ns target exists only as a comment in
`bench.rs` citing a file that does not mention it.

**Proper fix, unchanged but now specified.** `bench-history.py` should parse
this file and check each recorded entry against it, reporting unmatched names
in both directions as a failure rather than silence. That requires first
reconciling the names — one canonical name per benchmark, used by both the
`run()` call in `bench.rs` and the table here. The rename table above is the
work list. Until then the parse test added today guarantees only that the file
is *loadable*, not that it is *true*.


#### FOLLOW-UP 2026-08-14 (2): the file is now *checked*, and 11 targets disagree

`bench-history.py` gained `load_baselines()` + `report_baselines()`, which
compare the target the kernel prints on each `SCORE` line — the literal in
`bench.rs` — against the target this file states. The very first run of that
check, against `build/serial-test.txt` (63 benchmarks):

```
Baselines: 11 disagree, 15 unbaselined, 7 unused
  context_switch:      kernel says   5000ns, file says  10000ns
  crypto_aead_1KiB:    kernel says 100000ns, file says  70000ns
  crypto_sha256_1KiB:  kernel says  50000ns, file says  40000ns
  dns_build_query:     kernel says  40000ns, file says   2000ns   (20x)
  firewall_check:      kernel says   2000ns, file says   1000ns
  heap_alloc_free_64:  kernel says    400ns, file says    200ns
  http_mime_type:      kernel says   2000ns, file says    500ns   (4x)
  io_ring_nop:         kernel says    200ns, file says    300ns
  ipc_channel:         kernel says   2000ns, file says   3000ns
  page_fault:          kernel says  10000ns, file says   8000ns
  syscall_dispatch:    kernel says    200ns, file says   1200ns   (6x)
```

**Every PASS/OVER verdict for those 11 has been graded against a number its own
documentation contradicts.** The direction matters case by case: `syscall_dispatch`
measured 653 ns is *OVER* against the kernel's 200 ns and would *PASS* against
the file's 1200 ns. Which is correct is not obvious — 200 ns is the CLAUDE.md
hardware figure (Linux getpid ~100 ns, "within 2x"), while 1200 ns looks like a
TCG-adjusted budget. That is exactly why the check **reports and does not
reconcile**: picking a side automatically is how the two drifted apart.

The check distinguishes three failure modes deliberately, because they are
different problems: *disagree* (one side edited without the other), *unbaselined*
(the Rust literal is the only record of the target — 15 benchmarks, including
`vfs_stat_root`), and *unused* (the file claims coverage that does not exist — 7).
It also refuses to conflate an unparseable file with an agreeing one, printing
`UNVERIFIED`; that distinction is the entire lesson of this entry and is pinned
by a test.

Table renames brought name-matching from 30/63 to 48/63 (the tables moved, not
the benchmarks — `history.jsonl` is append-only and its names cannot change
without orphaning every historical record). 23 checks pass, up from 13.

**Still open:** the 11 disagreements need adjudicating one at a time, and the 15
unbaselined benchmarks need tables with real provenance. Both are now *visible on
every bench run* rather than invisible, which is the change that matters.

#### FOLLOW-UP 2026-08-14 (3): the 11 disagreements were mostly ONE bug — two kinds of target merged into one number

Adjudicating the 11 turned up a structural cause rather than eleven clerical
errors. `bench.rs` says it plainly in its own comments:

```rust
// OpenSSL SHA-256 1KiB: ~1500ns.  QEMU target: 50000ns.
score("crypto_sha256_1KiB", &result, 50000);

// DNS query build includes a heap allocation (Vec::with_capacity) which
// is expensive under QEMU (~35us).  Target set to 40us to track regressions
// without false-failing on the allocation overhead.
score("dns_build_query", &result, 40000);
```

**Those are TCG budgets, not hardware references** — and `baselines.toml` was
storing the hardware reference under the same key. Comparing them reported a
20x "disagreement" where in truth the two files were each right about a
different quantity. Two more (`heap_alloc_free_64`, `http_mime_type`) were the
same shape one level down: a *scope* difference, where the benchmark measures a
fixed multiple of the per-operation target (alloc+free is 2x an alloc; the MIME
benchmark does 4 lookups).

Worse, `bench-history.py` printed this on every run:

> *(The 'target' column in the scorecard above is a **hardware** reference and
> cannot be met under TCG — see bench/baselines.toml.)*

which is **false for at least six benchmarks**, whose targets are explicit QEMU
budgets. The line explaining the number misdescribed it, and so did the
scorecard headline: "48/63 within hardware target" counts passes that were
scored against TCG budgets.

**Fix: make the two kinds separate keys.** `target_ns` stays the hardware
reference; `tcg_target_ns` is the budget the suite is graded against under
emulation, and the cross-check prefers it when present. The explanatory line now
says the column is a mix and points at which key records which.

**Three were real disagreements.** Two are settled by CLAUDE.md's performance
table, which outranks the file:

* `context_switch`: file said 10 µs, spec says *"Target: < 5 µs"* → file corrected.
* `page_fault`: file said 8 µs, spec says *"Target: < 10 µs"* → file corrected.
* `ipc_channel`: file said 3 µs, spec says *"Target: < 2 µs round-trip"* → file corrected.
* `syscall_dispatch`: file said 1200 ns, derived by doubling a **638 ns WSL2
  measurement of a full syscall including spectre mitigations** — not the same
  quantity as dispatch. Spec says *"Linux: ~100 ns for getpid. Target: within 2x"*
  → 200 ns. **This one changes a verdict:** the measured 653 ns is OVER at
  200 ns and would have PASSed at 1200 ns. The 638 ns figure is kept as context,
  not as a derivation.
* `io_ring_nop`: file said 300 ns (2x a 150 ns measurement), spec says
  *"~100-200 ns per SQE; same order"* → 200 ns.

Result: **11 disagreements → 1.**

**The last one is instructive and is deliberately still open.** `firewall_check`
carries the comment `// Target from baselines.toml: 2000ns` in `bench.rs` while
the file says 1000 ns — a citation that is simply false, and the direction
(2x looser) means the kernel silently relaxed its own target at some point.
Both pass comfortably (measured 55 ns), so nothing is hidden by it; it is left
for the next `bench.rs` change rather than fixed now, because a kernel edit
during an in-flight release build would produce a binary that does not
correspond to any commit. Recorded here so it is not lost.

---


## FIXED (2026-08-15, lane C) — three workspace test failures from real-glyph measurement, two of them real bugs

`text::measure`/`text::wrap` now measure actual glyph advances instead of
estimating from byte counts. Three lane-C tests failed as a result. Only one
was a stale test; the other two were genuine rendering bugs the old estimate
had been hiding.

**1. `weather::an_alert_card_grows_to_hold_its_description` — stale test.**
`card_h = (ALERT_BODY_TOP + body_height + 12.0).max(90.0)`, i.e. `52 + 18N`
floored at 90, so growth is only observable at N≥3 lines. `LONG_ALERT` used to
wrap to 4 lines and now wraps to 2 at `text_width = 828` (app width 900 minus
padding), so the test compared 90 against 90. Fixed by building the input by
construction — `"Secure loose objects outdoors. ".repeat(40)` — and asserting
first that it actually wraps past the floor (`drawn > 2`) so the growth check
can never again silently compare the floor to itself.

**2. `wordsearch` — real bug: the strikethrough rule and checkmark overran the
word they annotate.** A word in the list is drawn with
`max_width: Some(140.0)`, but the rule's extent and the checkmark's x were
placed from the *unclipped* `text::measure`. A word longer than the column got
a rule running out past the clip into the grid beside it. Fixed by naming the
clamp (`WORD_LIST_MAX_WIDTH`, `WORD_LIST_FONT_SIZE`) and applying it to the
measurement that positions the marks:
`text::measure(...).min(WORD_LIST_MAX_WIDTH)`. The old test asserted
`bold < word.len() as f32 * 8.0 + 1.0` — a byte-count literal, which is both
fragile and wrong for non-ASCII; replaced with three postcondition tests
(rule matches the word drawn beneath it; ÉLÉPHANT measures within 10% of
ELEPHANT, i.e. by character not by byte; a 45-char word's rule never leaves
the column).

**3. `tmux` — real bug: a terminal grid sized from a proportional face.**
`char_width()` was `text::digit_advance(...)`, the advance of `'0'` in the UI
face: 7.55px at 13px, while `'W'` in the same face is 13.08px. Glyphs overhung
their neighbours' cell backgrounds and the block cursor sat beside the
character it marks. The root cause was that **the toolkit had no way to ask
for a monospace face at all.** Fixed by building that dimension end to end —
`osfont::system::Family { Ui, Mono }` on the cache key, `text::measure_in` /
`cell_advance` / `line_height_in` / `ascent_in`, `RenderCommand::PushFont` /
`PopFont`, `guiremote` tags `0x0B`/`0x0C`, a `font_stack` in the compositor —
and pointing tmux at it. See `design-decisions.md` §413 for why the family is
scoped render state rather than a field on all 4570 `Text` construction sites.

**The pattern all three share**, and the rule that would have prevented them:
a threshold test whose threshold is a *literal* and whose input is *measured
by the environment* degrades silently long before it fails loudly. Assert a
postcondition of the function (`w <= box_w`; "the rule matches the word drawn
beneath it") or build the input by construction (`.repeat(40)`) — never encode
a fact about the host's installed fonts.

**Latent hazard this leaves.** `text::digit_advance` still exists and is still
the wrong call for any terminal-shaped view; its doc now says so and points at
`cell_advance`. Any other app that lays out a character grid should be checked
for it.

---


## FIXED (2026-08-15, lane C) — the `digit_advance`-as-cell sweep: five more grid views

The tmux fix above named a hazard rather than an isolated bug: `digit_advance`
returns a digit's advance **in the proportional UI face**, which is a cell only
digits fit. Every caller using it to size a character grid had the same defect
latent, and `grep` found five more. All are now on `text::cell_advance` and
draw inside a `PushFont { Mono }` scope.

| Where | What it laid out on the wrong cell |
|---|---|
| `gui/toolkit/src/textview.rs` — `SimpleTextView` | Log/terminal output. Spans overran their own selection bands and search highlights; every column after the first drifted. |
| `apps/hexeditor` | The **ASCII column** — the earlier doc argued the grid was all hex digits and overlooked the column beside it, which draws whatever the bytes spell. `hit_test`'s `(ascii_x / char_w)` is this arithmetic run backwards, so a click resolved to the wrong byte, further wrong the further right it fell. |
| `apps/filediff` | The inline view's character-level highlight is placed at `columns(span) * char_width()`, so it slid off the very change it was drawn to mark. |
| `apps/markdowneditor` | The source pane's caret (`col_x`), selection band and find highlights drifted left of their characters, further with every wide glyph on the line. |
| `apps/snippets` | The token pen advances `columns(token) * char_width()`, so consecutive tokens on a line overlapped and indentation stopped lining up between rows. |

Each now carries two postcondition tests — every glyph of a sample set fits the
cell, in regular *and* bold (bold marks keywords, changed spans and headings on
the same grid) — plus a scope-balance test that walks the command list and
asserts the depth returns to zero, the scope was opened exactly one deep, and
glyphs were actually drawn **inside** it. That last clause is what stops the
test passing vacuously on an empty view.

**One caller was deliberately left proportional.** `RichTextView`'s
`char_width` looked like the same bug but is not: the widget was already
migrated to measure spans with `text::measure` and draw them proportionally,
and `char_width` survives only as the width of a gutter digit and the quantum a
list indents by. Both are UI-face quantities, so it now calls a separate
`default_indent_unit`, and the misleading "(monospace)" doc on the config field
is corrected. A test pins it to the UI face so the sweep cannot later "fix" it
into a regression.

**Remaining debt (not a bug, an enhancement).** `RichTextView` renders
`RichBlock::CodeBlock` in the proportional UI face like the prose around it.
That is self-consistent — the spans are measured in the face they are drawn in
— so nothing misaligns, but a code block *should* be mono now that the toolkit
can express it. Doing it properly means threading a family through
`span_width`, `x_of_col`, `col_at_x` and `wrap_spans` so the wrap is computed
in the same face the block is drawn in. The widget currently has **no callers
outside its own file**, so this is queued rather than urgent.


## `apps/installer` wrote unescaped strings into a GRUB config that runs at boot (lane C) — FIXED

`grub.rs`'s `generate_entry` interpolated every field of a `GrubEntry` —
`title`, `kernel_path`, `root_partition`, `uuid`, `initrd_path` and each of
`kernel_params` — straight into a `menuentry` block with no quoting and no
validation:

```rust
out.push_str(&format!("menuentry \"{}\" {{\n", entry.title));
...
out.push_str(&format!("    chainloader {}\n", entry.kernel_path));
```

That block is written to `/etc/grub.d/40_slateos` (mode 0755) and folded into
`grub.cfg` by `update-grub`. **GRUB executes `grub.cfg` at boot with full
firmware privilege — before any OS, and therefore before any OS-level security
boundary exists.** A title containing a `"` closes the string and everything
after it is parsed as fresh GRUB script; a title containing a newline does not
even need the quote. `$` expanded as a GRUB variable.

The reachability is the part worth remembering: this looked like a field the
user types into our own installer, so "who would attack themselves?". But
`os-prober` — the whole reason this module exists — *scrapes* menu titles out
of **other partitions'** `/etc/os-release`. On a dual-boot machine that is a
file the other OS controls, so the title is attacker-influenced input arriving
through a path that never looks like input.

**Fixed** by emitting every interpolated value inside `"…"` through a new
`grub_quote`, which escapes exactly the three bytes GRUB's lexer treats
specially inside a double-quoted string — `\`, `"`, `$` — mirroring
`grub_quote()` in GRUB's own `util/grub-mkconfig_lib.in`. Control characters
cannot be escaped that way, so `GrubEntry::validate` rejects them and
`generate_entry`/`generate_custom_script` now return
`Result<String, GrubError>`; `install`/`update` validate *before* touching the
filesystem, so a rejected entry leaves no file behind.

A second, non-security bug fell out of the same rewrite: `kernel_params` were
`join(" ")`ed into the line, so a parameter containing a space silently became
two parameters. Each is now quoted individually.

**Lesson, and it generalises past this file: "config file" is not a safe
output format.** The lossy-path sweep that led here trained the question *is
this value preserved byte-for-byte?* — but preservation is only half of it.
The other half is *can this value change the meaning of the document it is
written into?* A path can round-trip perfectly and still be an injection. Any
place we `format!` a value into a file that something else later *parses* —
GRUB config, shell script, YAML, JSON, a desktop entry — needs an escaping
function chosen for that grammar, not just faithful bytes. Worth auditing the
other generators in `apps/` on the same question.

Five separate defences, verified non-vacuous by breaking each one alone and
confirming it failed only its own test: escaping `$`, escaping `"`, escaping
`\`, the control-character rejection, and the per-parameter quoting.


## `gui/toolkit/src/svg.rs` named a character the author never wrote (lane C) — FIXED

`u8_from_hex_char`'s error did `c as char` on the offending byte. `c` is a
*byte* of the colour string and the bytes reaching that arm are exactly the
non-hex ones, which includes the continuation bytes of a multi-byte character:
`#ÿÿÿ` reported `bad hex char: Ã`, blaming a character absent from the input
and sending the author hunting for it. Now reports the byte (`bad hex byte:
0xc3`) for anything outside printable ASCII, and the character itself for
ASCII.

The other four `c as char` sites in this file were checked and are **correct**:
each sits in a match arm that has already matched `c` against ASCII byte
literals (or, for `cmd_char`, behind an `is_ascii_alphabetic()` guard), so the
cast is provably lossless there. Recorded so the next sweep does not re-open
them.


## Five copies of two escapers, at three levels of correctness (lane C) — FIXED

Following the GRUB finding above, the same question — *can this value change
the meaning of the document it is written into?* — was put to every generator
in `apps/`. It found five near-copies of a JSON escaper and two of an XML one,
which had drifted apart:

| Copy | JSON escaper | Verdict |
|---|---|---|
| `apps/jsonviewer` | `"` `\` `\n` `\r` `\t` `\b` `\f`, `\u00XX` fallback | correct |
| `apps/kanban` | as above (fixed in an earlier sweep) | correct |
| `apps/snippets` | `\u00XX` fallback present | correct |
| `apps/diagram` | five cases only, **no fallback** | emits invalid JSON |
| `apps/reminders` | five cases only, via `str::replace` | emits invalid JSON **and** corrupts on read |

**`apps/reminders` was the serious one.** Its `unescape_json` was a chain of
`str::replace` calls in the wrong order — `\n` decoded before `\\`:

```rust
s.replace("\n", "\n").replace("\r", "\r").replace("\t", "\t")
 .replace("\\\"", "\"").replace("\\\\", "\\")
```

So the two-character text `\n` (a literal backslash, then the letter n) was
escaped to `\n` on save and read back as a **newline**. A Windows path in a
note, `C:\temp`, came back as `C:\<TAB>emp`. The damage was then re-saved, so
the note decayed a little further every time the app was opened. The existing
test `test_json_escape_special_chars` covered this function and passed,
because its sample text — `"Hello \"world\"\nnew line"` — contains a real
newline and real quotes but not one literal backslash, the single input that
tells a correct decoder from a broken one.

**`apps/whiteboard` had an unescaped XML export**: `page.name`, `layer.name`
and both `TextLabel` and `StickyNote` content went straight into the markup, so
a sticky note reading `</sticky><rect/>` closed its own element and injected a
sibling, and any `&` made the export unparseable. Same class as the GRUB bug,
found by the audit that bug prompted.

**Fixed** by adding `gui/toolkit/src/escape.rs` (`guitk::escape`) with one
correct implementation of each — `xml`, `json_string`, and a
`unescape_json_string` that is a single left-to-right pass and so structurally
cannot make the replace-chain mistake — and routing `reminders`, `whiteboard`,
`diagram`, `snippets` and `markdowneditor` through it. Non-vacuity verified by
breaking each of the five defences alone; each failed only its own tests.

**Not converged, deliberately:** `apps/kanban` and `apps/jsonviewer` decode
inside full tokenising JSON parsers (`parse_string(data, start) -> (String,
usize)`), a different shape from a standalone `unescape`. Both are already
correct, so rewriting them onto the shared helper would risk regressing working
code for no correctness gain. If a third parser of that shape appears, extract
a shared *parser* rather than bending these two into the wrong signature.

**The generalisation, now twice-confirmed:** a value can be preserved
byte-for-byte and still be a bug. The lossy-path sweep asked *is this
preserved?*; this one asks *can this re-punctuate its document?* Every
`format!` into a file that something else later parses needs an escaper chosen
for that grammar. Remaining unaudited generators of this kind: the YAML and
`.desktop`-style writers, if any, and `pkg/`'s manifest output.


## Data exporters: CSV/JSON/SQL injection in `netscan`, `credmanager`, `dbviewer` (FIXED)

Third pass of the "a config file is not a safe output format" audit, covering
the tabular exporters. Four distinct defects, all the same shape:

**`apps/netscan` did no CSV escaping at all.** This is the worst of the four
because the inputs are not the user's: a `hostname` comes from reverse DNS and
a `service`/`banner` from banner grabbing, so both are chosen by the *scanned*
host — on a scan, precisely the party with no reason to be trusted. A comma in
a hostname added a column and a newline added a whole row, letting a hostile
host forge result rows for machines that were never scanned. The hand-written
`"{}"` around the port/service columns was not a defence either: it never
doubled an internal quote, so a `"` in a service name closed the field early.
Its JSON export had the same holes plus a banner escaper handling `"`, CR and
LF but *not* the backslash — a banner ending in `\` produced `"...\"`, an
unterminated string that truncates the document.

**`apps/credmanager` left `tags` and `folder` raw** in the CSV (the only two
of nine columns not escaped), its `escape_csv` omitted `\r` from the trigger
set (RFC 4180 records are CRLF-terminated, so a bare CR splits the record for
most readers), and `serialize_backup` escaped *nothing* — vault name, entry
name, tag and folder names all interpolated bare. For a credential vault that
is the worst possible failure: a `"` in any name yields a backup file that no
reader can load, i.e. a silently unrestorable backup.

**`apps/dbviewer` escaped every value in all three exporters and no column
name in any of them.** The corollary this pass added to the audit question:
*audit the field names, not just the field values.* Column names are not
privileged data — `import_csv` takes them straight from the header line of a
file the user opened. Also `export_json`'s `s.replace('"', "\\\"")` (escaping
the quote but not the backslash, worse than useless for a value ending in `\`)
and `export_sql_inserts` interpolating table/column names as bare SQL
identifiers.

**`apps/dbviewer`'s importer could not read its own exporter's output.**
Found while fixing the above. `import_csv` split the header with a naive
`split(',')` and iterated `csv_data.lines()`, so a quoted field containing a
comma (header) or a newline (any record) was torn apart — even though
`parse_csv_line` underneath it was correctly RFC 4180-aware for data rows.
Fixed properly by replacing both with one record-level `split_csv_records`
that never splits on a line boundary before it knows whether it is inside
quotes. It also now trims only *unquoted* fields: quoting is how a writer says
the surrounding whitespace is data. Locked in by a round-trip test.

**Fixed** by adding `guitk::escape::csv_field` (RFC 4180, trigger set
`, " \n \r`) to the shared module, a local `sql_ident` in `dbviewer` (standard
double-quote identifier quoting), and routing all of the above through them.
Non-vacuity verified by breaking each of the nine defences alone; each failed
only its own tests.

**A testing note worth keeping.** Three of the new tests failed on first run
*because the tests were wrong, not the code* — each had counted a naive
substring. Correctly escaped output legitimately *contains* the payload:
`\", \"admin` contains `"admin`, a quoted CSV field contains a comma and a
newline, and a quoted SQL identifier contains a `;`. A test for an injection
defence therefore cannot use `contains`/`split`/`lines` — it has to decode the
way a conforming reader does. The fix in each case was a small escape-aware
scanner (`parse_csv`, `json_string_token_count`, `sql_statement_count`) living
beside the tests. This is the same trap as the GRUB `menuentry ` substring
count from the first pass; it has now appeared in all three passes, so treat
"count the tokens a parser would see" as the default shape for these tests.


## `guitk::csv`: a format's writer and reader belong in one module (FIXED)

`apps/spreadsheet` turned out to have the *identical pair* of defects
`apps/dbviewer` had: an `export_csv` whose quoting trigger set omitted `\r`,
and an `import_csv` that split records with `csv.lines()` before handing each
line to a perfectly correct, quote-aware field parser. Both apps could
therefore produce an export they could not themselves read back — a quoted
cell containing a newline was torn in half and the rest of its row dropped.

Two independent apps making the same two mistakes is the signal to stop
patching and restructure, so the CSV format now lives in one module,
`gui/toolkit/src/csv.rs`, holding **both** directions: `csv::field` (write)
and `csv::parse_records` (read). Keeping them adjacent is the point — the
whole bug class is a writer and a reader drifting apart, and it is much harder
to write a line-splitting reader thirty lines below an escaper that
deliberately emits newlines inside fields.

`csv_field` moved out of `guitk::escape` in the process. Escaping a CSV field
is not a standalone escaping problem the way XML or JSON escaping is; it is
half of a codec, and filing it under "escape" is what made it natural to write
the other half somewhere else. `escape` keeps a comment pointing at `csv`.

`Field { text, quoted }` reports whether the source spelled a field in quotes,
because the two apps disagreed on trimming and both were right: `dbviewer`
wants the lenient "trim a bare field" import convention, `spreadsheet` wants
cells verbatim. Quoting is exactly the writer's statement that the surrounding
whitespace is data, so `Field::trimmed_if_bare` lets a caller be lenient
without corrupting a deliberately-padded value. Locked in by a round-trip test
in each app plus `anything_written_can_be_read_back` in the module itself.

Both apps' local parsers were deleted rather than left in place; a weaker
second parser sitting in the file is the thing that gets reached for next
time.


## `apps/musicplayer`: ID3 tags could forge M3U playlist entries (FIXED)

`export_m3u` interpolated `track.artist` and `track.title` straight into the
`#EXTINF:` line. Those two fields are not the user's: `Track::update_from_data`
sets them verbatim from the file's own ID3v2 tags, so for any downloaded file
they are chosen by whoever produced it. `load_m3u` reads every non-`#` line as
a **file path**, so a title containing a newline injected arbitrary entries
into the user's playlist.

M3U is where this audit's usual answer runs out: the format is bare
line-oriented text with no quoting and no escape syntax, so a line break
cannot be escaped — only removed or refused. The fix splits on which of those
is honest for each field:

- `#EXTINF` metadata is advisory display text, so CR/LF become a space
  (`m3u_field`). Losing a newline out of a song title costs nothing.
- A **path** containing CR/LF is legal on this OS (all bytes but `/` and NUL)
  and has no M3U representation at all. Writing it anyway would silently point
  the entry at a different file, so the track is omitted — and *reported*:
  `export_m3u` now returns `M3uExport { text, skipped }` instead of a bare
  `String`, so a caller can tell the user rather than handing them a playlist
  quietly shorter than the one they exported.

The general point, third variant of it now: when a format cannot represent a
value, the choice is reject or sanitise, and it must never be "write it
anyway." GRUB got reject (control characters), M3U metadata gets sanitise, M3U
paths get reject-and-report.


## `apps/contacts`: a chained-`replace` decoder corrupted every note containing a backslash (FIXED)

**Status: FIXED 2026-08-15** (lane C). Found while auditing the vCard/iCalendar
family during the output-escaping sweep. This is the same defect previously
fixed in `apps/reminders`, in its third instance, and this time the *correct*
implementation was already sitting in the neighbouring app.

`vcard_unescape` decoded with a chain of `str::replace`:

```rust
s.replace("\n", "\n")     // <-- runs first
 .replace("\,", ",")
 .replace("\;", ";")
 .replace("\\\\", "\\")    // <-- too late
```

`vcard_escape` correctly writes the two-character text `\n` (a backslash
followed by the letter n) as `\n`. The decoder then scans that for the
sequence backslash-n, finds it at offset 1, and emits a real newline. So
`C:\new` came back as `C:\`, a line break, and `ew`.

The trigger is ordinary content, not a crafted one: a Windows path, a regex, a
LaTeX fragment, a `\server\share` UNC name — anything a person might
reasonably paste into a contact's NOTE field.

**The corruption happens once, on the first load, and is then a fixed point** —
re-saving does not degrade it further. That is worth stating precisely because
it makes the bug *quieter* rather than milder: the damaged value is what gets
written back, so after a single load-and-save cycle the original text is gone,
and there is no accumulating drift to make the loss noticeable. A test that
looked only for unbounded growth would have passed.

Fixed with a single left-to-right pass that consumes the backslash and the
character after it together. Such a pass structurally cannot make this mistake,
because it never re-examines output it has already produced — the ordering
question that a `.replace()` chain has to answer correctly simply does not
arise.

Two things came out of the cross-check that are worth recording:

- **`apps/calendar::ics_unescape` was already correct**, single-pass, and
  carried a comment naming this exact anti-pattern. The same format family held
  one correct and one broken implementation of the same rules, a few hundred
  lines apart in a sibling crate — which is the duplication problem the
  `guitk::csv` extraction was about, showing up in a format that has not been
  extracted yet.
- **`vcard_escape` also passed a bare CR through untouched.** vCard has no
  escape for CR and its lines are CRLF-terminated, so a CR in a value ended the
  property line early and the remainder was parsed as a new property — a note
  could forge a `TEL:` line. Fourth instance of "the format cannot represent
  this value, so reject or sanitise": here it sanitises, because a CR in a text
  field means a line break, and a CRLF pair now yields one break rather than
  two.


## `apps/email`: every outgoing header was interpolated raw — header injection (FIXED)

**Status: FIXED 2026-08-15** (lane C). The most serious defect the output-escaping
audit has turned up, and the one whose consequence is least visible to the user.

`EmailDraft::build_message` wrote every header value straight into the message:

```rust
msg.push_str(&format!("Subject: {}\r\n", self.subject));
msg.push_str(&format!("To: {}\r\n", self.to.join(", ")));
msg.push_str(&format!("In-Reply-To: <{reply_to}>\r\n"));
msg.push_str(&format!("Content-Type: {}; name=\"{}\"\r\n", att.mime_type, att.filename));
```

RFC 5322 gives a header field no way to contain a line break. The field *ends*
at CRLF; folding — a CRLF followed by whitespace — is a continuation the
serialiser chooses, not something a value can request. So a CR or LF in a value
is not escaped, it **terminates the header**, and the receiving MTA reads what
follows as a header of its own.

A subject of `Lunch?\r\nBcc: mallory@evil.test` therefore adds a recipient. The
reason this is worse than an ordinary injection: **the forged Bcc appears
nowhere the sender can see it** — not in the compose window, which shows the
subject field as typed, and not in the Sent copy, which is rendered from the
same draft object. The mail silently goes somewhere the user cannot discover it
went.

### What was and was not reachable

Worth stating precisely, because the inbound side turned out to be sound and
that is a design worth not regressing.

- **Not reachable: anything parsed off the wire.** `Headers::parse` unfolds
  continuation lines into spaces, so no value read from a received message can
  carry a CR or LF. That closes what would otherwise be the nastiest path:
  `EmailDraft::reply` copies the original's `Message-ID` into `In-Reply-To`, so
  a hostile `Message-ID` would have been injected into the victim's reply with
  no interaction beyond pressing Reply. The unfolding is what prevents it, not
  anything at the serialiser, which is why the serialiser now sanitises anyway.
- **Reachable: everything composed locally** — the subject and recipients the
  user types or pastes, and attachment filenames. The filenames matter more
  here than on other systems: `design.txt` allows every byte except `/` and NUL
  in a path, so **a newline in a filename is legal on SlateOS**. A downloaded
  file can carry one, and attaching it forged headers.

### Also fixed: the boundary was a constant

The multipart delimiter was the literal `----=_Part_Boundary_001`. RFC 2046
requires the boundary to appear nowhere inside an encapsulated part, and a fixed
string cannot promise that. A body containing it — which a user produces just by
quoting a previous multipart mail — ends the part there, and **every attachment
below that point silently disappears from the sent message**. The boundary is
now derived from the body, lengthening on collision; this terminates because a
finite string contains no arbitrarily long substring, and the first candidate is
the old constant, so ordinary mail is byte-identical.

### The shape of the fix

Five helpers, chosen per field by what the grammar can express and by whether
the field is advisory or load-bearing — the reject-or-sanitise rule this audit
keeps arriving at, now on its fifth format:

| Field | Grammar offers | Treatment |
|---|---|---|
| `Subject`, display names | nothing | sanitise: control characters → space |
| `To`/`Cc`/`Bcc` | nothing | **reject and report** — a recipient decides where the mail goes, so a bad one must not be quietly rewritten into a different address |
| `Message-ID`, `Content-ID` | nothing | sanitise: drop controls, `<`, `>`, whitespace |
| attachment `filename` | `\"` and `\` inside a quoted-string | escape quote and backslash; drop controls |
| attachment `Content-Type` | nothing (it is a token) | **fall back** to `application/octet-stream` — a mangled media type is not a media type, so there is nothing to sanitise it *into* |

`build_message` now returns `BuiltMessage { text, rejected_recipients }` rather
than a bare `String`, for the same reason `export_m3u` returns skipped paths: a
dropped recipient is exactly what the sender must be told about, and a function
returning a `String` has nowhere to say it.

### Two lessons from the break-testing, not the fix

Breaking each defence in turn to check the tests notice found that **two of the
new tests could never have failed**, which is worth recording because both
mistakes are easy to repeat:

1. The header-scanning helper stopped at the first blank line — correct for the
   top-level block, but a **MIME part has its own headers after that blank
   line**, so the test for a forged header in an attachment filename was
   inspecting a region the payload never reached.
2. It split only on `\r\n`. Real receivers are lenient and many end a line at a
   bare LF, so a test that only recognises CRLF is *stricter than the attacker*
   and passes on genuinely vulnerable output.

Both fixed by scanning every line terminator across the whole message and
counting lines that *begin* with the header name. Counting line starts rather
than substrings is what keeps it honest in the other direction, and is the same
point the CSV and SQL tests reached: correctly quoted output legitimately
contains the payload text, so `contains` cannot be the assertion.

The display-name test still fails under no single break, because the value is
covered by two independent defences; breaking both together does fail it, which
is how it was confirmed to be defence in depth rather than a vacuous test.


## slides: one HTML export field skipped the escaper (fixed 2026-08-15, lane C)

`apps/slides`'s `export_html` escapes the presentation title, text-box bodies
and bullet items, and does so correctly. It did not escape the placeholder
label of an `Image` element, which is user-typed and is written straight into
the exported document. A label of `<script>…</script>` — or, more cheaply, a
`"` closing the `style` attribute early — is therefore reproduced as markup by
any browser opening the export.

### Why this one and not the other three

The three fields that were escaped each sit in a statement of their own:

```rust
push_html_escaped(&mut html, &slide.title);
```

The one that was not was a `{}` inside a larger `format!`, in the company of
five geometry values that genuinely cannot need escaping:

```rust
html.push_str(&format!(
    "  <div class=\"img-placeholder\" style=\"left:{x}px;top:{y}px;\
     width:{width}px;height:{height}px;\">{placeholder_label}</div>\n",
));
```

Reading that line, the eye is doing arithmetic, not taxonomy. Every other name
in the interpolation is an `f32`, and `placeholder_label` inherits their
apparent harmlessness by proximity. This is the recurring shape of the whole
audit: the dangerous interpolation is rarely the one on a line by itself — it
is the one *embedded among values that are obviously safe*, where the reader's
attention has already been spent. A grep for `format!` finds it; a reading of
the function does not.

The fix splits the statement so the label goes through `push_html_escaped` like
its three siblings, which also makes the asymmetry impossible to reintroduce
without deleting a call.

### Test

`no_text_field_can_inject_a_tag_into_the_export` drives *one* payload through
all four text fields at once — title, text box, image label, bullet item — and
counts tags rather than substring-matching, since escaped output legitimately
contains the payload text. Driving every field from a single payload is what
makes the test grow with the exporter: a fifth text field added later either
routes through the escaper or fails this test. A second test checks the
attribute case specifically, since a bare `"` escapes the `style` value without
needing a `<` at all.


## clipmanager, flashcards, mindmap: three exporters that could not read themselves (fixed 2026-08-15, lane C)

The same audit, three more apps. All three wrote user text raw into a
line-oriented format whose structure is made of characters that text can
contain. Two of them have importers, so both could produce an export they
themselves misread; the third has no importer, which changes who the victim is
but not whether the bug is real.

### clipmanager — the worst of the three, because of what the field holds

`export_text` wrote the clip content raw after a bare `content:` line, and
`import_text` recovered records with `data.split("---ENTRY---")` — a *substring*
split, not even a line match. So a clip containing that marker split its own
record in two, and the second half's lines were then parsed as **headers**,
letting copied text set its own `source:` and `pinned:` and add tags.

What makes this the severe one is not the mechanism but the field. A clipboard
entry is arbitrary copied text — the one value in the whole desktop guaranteed
to hold whatever the user last selected in a browser. Every other app in this
audit needed the user to type the payload into a name or a note; here they only
have to copy it.

Escaping the body would have worked and would have been wrong. The point of
this format is that you can open it and see what you copied; an escaped
multi-line body is unreadable. The fix is a **length prefix**:

```
content:<byte length>
<exactly that many bytes>
```

Bytes inside the body are then never examined, so no sequence in them means
anything — a stronger guarantee than escaping, and a cheaper one to verify.
The parser became a single left-to-right pass, necessarily: the body length is
only known once its header has been read, which `split` could not have
consulted. Header values (`source:`, tags) are sanitised so they stay on their
own line, and tags get a line each instead of a comma-joined list. The format
now needs no escaping anywhere.

A round-trip defect surfaced from the new tests, unrelated to injection:
export wrote newest-first while import replays through `add`, which prepends,
so **importing your own export reversed your clipboard history**. The file is a
log, so it is now written oldest-first. Worth noting that the existing
round-trip test did not catch this — it checked the count, not the order.

### flashcards — the failure mode is pedagogical, not technical

Every structural signal in the deck format is a character card text can
contain: the `Q:`/`A:`/`T:` prefixes, the blank line that ends a card, the
comma between tags, the line break itself. A question written the obvious way —

```
What is 2+2?
A: 5
```

— exported and re-imported as *two* cards, one of them with an answer its
author never wrote. This is the entry in this audit whose consequence is
strangest: nothing crashes, nothing is exfiltrated, and the user revises from
the deck and learns the wrong thing.

Fixed with the backslash escaper and matched single-pass decoder from the vCard
work. Two decisions differ from that one, both because this format is ours
rather than a published spec:

- **Commas are escaped in tags only.** Flashcard questions are full of commas;
  turning `What is 2, 3, and 4?` into `What is 2\, 3\, and 4?` would wreck a
  format that is meant to be hand-editable for no gain, since a `Q:` line has
  no comma-separated structure to protect.
- **CR gets its own escape** rather than being folded into `\n` with the LF
  beside it. vCard *has* to normalise — its spec says a line break is spelled
  `\n` and nothing else. Here nothing forces that, so escaping CR separately
  makes the round trip exact rather than faithful-in-spirit, and leaves no
  lossy corner to document.

Two further round-trip losses fell out: the importer trimmed each line before
matching the prefix, so leading and trailing spaces in a value were lost, and
an empty value (`Q: ` with nothing after it) failed the `strip_prefix("Q: ")`
and **dropped the card entirely**. It now matches the raw line and falls back
to the trimmed one, which keeps the leniency for hand-written decks while
making the app's own output exact.

### mindmap — no importer, so the reader is a person

`export_node_text` wrote node labels raw into an indented outline, where
structure *is* whitespace: a newline starts a sibling and the leading spaces
choose its depth. A label containing a line break therefore draws branches in
the exported map that do not exist in the real one.

There is no importer, which is worth stating precisely rather than using as a
reason to skip it: the absence of a parser does not make the output correct, it
only changes who is misled — a human reading the outline, or whatever other
outliner they open it in. Labels are short prose with nothing to escape *with*,
so this one is a sanitise: control characters fold to single spaces and runs
collapse, keeping the label a readable phrase.

### On the break-testing, again

Every defence added here was broken individually to confirm its tests notice —
twelve breaks across the three apps. Two findings worth carrying forward:

1. **A test can be vacuous by being one character short of the real attack.**
   The flashcards deck-name test passed against *unescaped* output on its first
   version, because the payload `"Name\nQ: forged\nA: forged"` has no trailing
   blank line — and a card is only committed by the blank line that ends it, so
   the forged pair was silently overwritten by the next one. The defence was
   real; the test was not exercising it. Only breaking the fix on purpose
   revealed the difference.
2. **A defence can be genuinely redundant, and that is fine as long as it is
   labelled.** In clipmanager, matching the record marker as a whole line
   rather than a substring is unreachable *inside* a record once the body is
   length-prefixed. Rather than delete it or write a test that cannot fail, the
   case that does reach it was found — the scan for the *first* record runs over
   whatever preamble the file has, such as a covering note that mentions
   `---ENTRY---` in a sentence — and the test drives that.


## indexer and fileassoc: config files whose values could re-punctuate them (fixed 2026-08-15, lane C)

The same audit again, on the two remaining `key = value` config formats. Both
bugs are silent-wrong-result rather than crash-or-corruption, and one of them
defeats a security control.

### indexer — a comma in a path defeated an exclusion

`/etc/indexer.conf` stored `index_paths`, `exclude_paths`,
`include_extensions` and `exclude_extensions` as **comma-joined** lists. On
this system a path may contain any byte but `/` and NUL, so a comma is an
ordinary filename character. Excluding `/home/u/Private, Ltd` wrote one line
that read back as *two* entries — `/home/u/Private` and `Ltd` — neither of
which named the directory the user meant. The directory was therefore not
excluded, and with `index_contents` on, its contents were read into a
searchable index.

That is the part worth stating plainly: `exclude_paths` is not a preference,
it is the mechanism by which a user keeps a directory out of a system-wide
search index. A format that cannot represent the user's answer is a format
that silently overrides it.

Fixed by giving each entry **its own line** — `index_path = …` repeated —
rather than escaping the comma. Escaping a comma works; a separator that never
appears is better than one that is escaped correctly, and it keeps an ordinary
config readable. The plural keys still parse for hand-written files, and the
first repeated key clears the built-in defaults so a config can shrink the list
and not only grow it. A related loss fell out of the tests: an explicitly empty
list used to reappear as the built-in defaults on the next read.

### fileassoc — the exporter and the importer disagreed, and nothing said so

`from_config_line` trimmed both halves; `export_config` wrote the raw strings.
An extension registered as `"txt "` is registerable — `register_file_type`
validates nothing and `set_default_app` only lowercases — so it exported as
`txt =myapp` and read back as `txt`, **silently reassigning a different
extension's default application**. No error is reported on any path: the line
parses, the extension exists, the app exists and supports it, so every
validation the importer performs passes.

`#` had the same shape in the other direction. A comment line is skipped
entirely, so an extension of `#txt` exported to a line the importer discards,
losing the association without a word.

Both are fixed by escaping through `textfmt::kv` with `=` and `#` named as the
grammar's structure characters, and by having `export_config` call
`Association::to_config_line` instead of keeping a second copy of it inline.
That second copy is the real lesson here: the writer and the reader were
*already* a matched pair on `Association`, and the drift happened because
`export_config` bypassed the writer and open-coded the format a third time. A
format with two writers has no invariant, only a coincidence.

### The band-aid, and where the escaper now lives

By fileassoc this was the fourth app in a row needing the same line-value
escaper, and the third place it had been written inline. Per CLAUDE.md's rule
about band-aid accumulation, it was extracted rather than copied again.

The extraction was not to `guitk`, where `csv` and `escape` already lived. The
components with the strongest need for these primitives turn out to be exactly
the ones that must not depend on a widget library: `apps/backup`,
`apps/indexer` and `apps/installer` are headless, and are three of the four
`apps/` crates with no `guitk` dependency. Unable to reach the shared
escapers, each had grown its own — which is the whole mechanism by which the
duplication happened. So the modules moved to `textfmt`, a dependency-free
`no_std` crate alongside `yamldoc` and `tzrules`, and `guitk` re-exports them
under their original paths so the 137 applications that say `guitk::csv` did
not have to change.

Two invariants are now documented in one place instead of being rediscovered:

1. **Decode in a single left-to-right pass, never a chain of `str::replace`.**
   Undoing `\n` before `\\` turns the two-character text `\n` — a legal
   directory name here — into a real newline. A single pass structurally cannot
   make that mistake, because it never re-examines what it has produced.
2. **An escape must not end in whitespace.** These parsers trim the value,
   which is the right leniency for a hand-edited file, but it means writing a
   trailing space as `\ ` leaves the file ending `...\`, which decodes to a
   stray backslash. Hence `\s`.

### Break-testing

Five breaks on fileassoc, each caught by a named test: removing the escape on
write, the unescape on read, the escape-aware split, `#` from the meta
character set, and routing `export_config` around `to_config_line`. That last
one is the break that reproduces the original bug exactly, and it is worth
keeping precisely because it will fail again the moment someone re-inlines the
format for convenience.


## devicemanager: a USB device could forge a section of the hardware report (fixed 2026-08-15, lane C)

`export_report` interpolated eight device-supplied strings raw — name, vendor,
type, hardware ID, location, and the driver's name, version and provider — into
a report whose structure is line breaks, `--- Section ---` headers and
two-space indentation.

What makes this one worth its own entry is where the strings come from. They
are not typed by the user; they are read off the hardware. A USB device chooses
its own product and manufacturer descriptors, and nothing in the descriptor
format constrains their content or forbids a line break. So a device that calls
itself

```
Mouse
--- Storage ---
  Fake Disk [OK] (ACME)
```

writes a whole forged section into the hardware report of any machine it is
plugged into — a report whose entire purpose is to be trusted when someone is
diagnosing that machine, and which is typically pasted into a bug report or
handed to whoever is helping.

There is no importer, which is worth stating precisely rather than using as a
reason to skip it: the absence of a parser does not make the output correct, it
only changes who is misled — here a person, or whatever they paste the report
into.

Fixed with a fold, not an escape. The choice follows from the reader: there is
nothing to undo an escape, so a literal `\n` in the output would be noise to a
human where a real newline is a forgery. Every control character becomes at
most one space, runs collapse, and edge space is dropped so a name padded with
spaces cannot appear to sit at a different depth in the report's indentation.

### flashcards' last lossy corner is closed

Migrating flashcards onto the shared `guitk::kv` was meant to be deduplication
— the fourth inline copy of the same escaper — but it also closed the deck
format's one documented limitation. `split_tags` trims each tag, which is the
right leniency for a hand-written `T: math, algebra`, but the trim reached the
*value*, so a tag of `" spaced "` came back as `"spaced"`. `kv` writes an edge
space as `\s`, which is not a space: the trim cannot find it, and the decode
happens afterwards. The trim still does its job — absorbing the layout of a
hand-written list — without being able to reach the data.

Worth noting as a general point: three of the four apps migrated onto the
shared escaper gained a fix they were not migrated for. Consolidating on one
correct implementation is not only less code; it retro-actively repairs every
corner each local copy had quietly given up on.

### A substring count is a `contains` in disguise

The first version of both devicemanager tests failed against the *fixed* code,
and the tests were what was wrong. They asserted
`report.matches("--- ").count() == baseline` and
`!report.contains("--- Forged ---")`.

But a correctly folded name still carries every character of its payload —
`--- Storage ---` is right there in the output, now harmlessly mid-sentence.
This is the same lesson already recorded for the escaping work ("count records,
never `contains`, because correctly escaped output legitimately *contains* the
payload") arriving in a disguise that got past it: a substring *count* looks
quantitative and structural, and is neither.

The guarantee a fold actually provides is positional, so the assertion has to
be too. Every interpolated field is preceded on its line by the report's own
indentation, therefore no field can begin a line, therefore none can *be* a
header. The tests now count lines that satisfy `starts_with("--- ") &&
ends_with(" ---")`, plus the report's total line count. Both survive breaking
each of the eight fold sites individually.


## sysinfo: an environment variable could write a heading of the system report

`apps/sysinfo/src/main.rs`. Fixed in `dab9fab26`. Two bugs of one cause, and
the cause is the interesting part.

`export_text` writes a report whose grammar puts headings at column 0 and data
indented by two spaces. It chose between them like this:

```rust
} else if prop.value.is_empty() {
    out.push_str(&format!("{}\n", prop.name));   // column 0
} else {
    out.push_str(&format!("  {}: {}\n", ...));   // indented
}
```

The empty-value branch exists so the file can emit its own sub-headings —
`Property::new("--- CPU Features ---", "")`. But `props_env_vars` builds a
`Property` directly from each environment pair, and `FOO=` is a legal and
ordinary environment variable. So a variable named `--- Display Outputs ---`
with an empty value printed itself at column 0, byte-identical to the heading
the report writes for the display section.

**This one needed no control characters at all.** Every other finding in this
audit required the payload to smuggle in a newline; folding was therefore a
complete fix for them. Folding does nothing here — there is nothing in the
string to fold. That is worth remembering as a class: *a value can forge
structure without containing any structural character, if the format infers
structure from something other than the value's text.* Here the inference was
from the value's **emptiness**.

The detail-pane renderer had the same bug in its own dialect:

```rust
let is_section = prop.name.starts_with("---");
```

so a variable named `---x` was drawn bold and in the accent colour.

### The fix, and why it is not "escape the name"

Two consumers were each re-deriving *is this row structure?* from the strings.
The strings are environment variables, PCI vendor names and process names —
the one place the answer cannot live. Escaping or folding the name only
narrows the set of strings that happen to fool the inference; it leaves the
inference.

So the distinction is now recorded at construction by the code that knows it:
`PropertyKind::{Heading, Blank, Field}`, with `Property::heading` for the three
sub-headings this file writes, `Property::blank` for the ten separators, and
`Property::new` for data. `Field` rows are always indented — including when
their value is empty, which now means nothing beyond an empty value.

This is the same shape as the fileassoc finding recorded above ("a format with
two writers has no invariant, only a coincidence"), reflected: there, one
format had two *writers* that drifted; here, one format had two *readers* both
inventing an invariant that was never written down.

`Property::new` folding both halves is a second, independent benefit: it closes
the ordinary newline vector for all fourteen `props_*` functions at once —
PCI descriptions, driver paths, process names — rather than at each call site.

### Multiplicity is the new position

sysinfo had no unit tests; there are now seven. The headline one did not catch
its own bug on the first draft, and the reason is the same lesson as
"a substring count is a `contains` in disguise" wearing yet another disguise.

It forged headings that duplicate ones the clean report already contains —
deliberately, because a forgery *identical* to a real heading is the strongest
form of the attack. It then asked, of each column-0 line in the hostile report,
"is this a line the clean report also produced?" The answer was yes, and it
passed.

The assertion has to compare column-0 lines as a **multiset**. Set membership
discards multiplicity exactly as `contains` discards position. Running the
break — reinstating the emptiness guess — now fails
`an_empty_environment_variable_is_not_a_section_heading`, which is the test
named after the bug; before the fix only a bystander test caught it.

Three breaks were run against the final code (reinstate the emptiness guess;
stop folding in `Property::new`; make `Property::new` return `Heading`). All
three are caught, each by at least two named tests.
