## §203 — A benchmark that is not recorded must say so out loud: every measurement window is either scored, tracked, or *declared* a diagnostic

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel benchmark suite prints ~90 timing measurements per
`--bench` boot, but only some of them get filed into `bench/history.jsonl`, which
is the file the regression comparator reads. The rest are printed to the serial
log and forgotten — no history, so nothing can ever notice them getting slower.
Nobody knew which were which. This records the rule adopted: **every measurement
must be classified**, and the suite now prints a line each boot naming any
measurement that hasn't been.

**Terms:** *score* = file the measurement with a target to grade it against.
*track* = file it with no target, so it is compared to previous boots but never
graded pass/fail. *diagnostic* = a number that is deliberately not filed, because
it means nothing on its own. *window* = one measurement — one call to the
harness's `run()`.

**The decision.** Three destinations, and a measurement must reach exactly one:

| | Filed to history? | Graded? | For |
|---|---|---|---|
| `score(name, r, target)` | yes | yes | a benchmark with a defensible published or measured target |
| `track(name, r)` | yes | no | a benchmark worth comparing run-over-run with nothing to grade against |
| `run_diagnostic(name, …)` | no | no | a number that is only meaningful *relative to another number in the same block* |

The coverage line at the end of the suite reports the three counts plus a fourth
— `unjudged` — and names every unjudged window. `unjudged` is expected to be 0
forever; anything else is a measurement whose author has not said which of the
three it is.

**The tradeoff, which is real in both directions.**

*Against having a `diagnostic` category at all:* it is an opt-out from
regression detection, and opt-outs get used to silence inconvenient reports.
Every window `run_diagnostic` covers is a number that can now degrade forever
unwatched. The maximally-safe design is "record everything" — `track` costs
almost nothing, and the comparator can always be told to ignore an entry later.

*For it:* recording a decomposition stage does not give you regression detection
on it, it gives you *noise*. `sd_ktrace_pair` measures two ktrace calls in
isolation so they can be subtracted from the syscall-dispatch total; its absolute
value is meaningless, it swings with TCG scheduling, and a comparator watching it
would fire on nothing. Worse, the eleven decomposition sub-measurements would
have been ~13% of the record, all of them noisy, which degrades the
signal-to-noise of the whole file. And the alternative to a `diagnostic`
category is not "record them" — it is *the status quo*, where they were
unrecorded **and unmarked**, indistinguishable from a benchmark someone forgot.

That last point is what settles it. The category is not licence to skip work;
it is the mechanism that makes skipped work *visible*. Before it, the coverage
report could only ever say "21 windows unrecorded" and would say it every boot
forever, which is the shape of a check people learn to ignore — the same failure
as an assertion that fires on every healthy boot. With it, the report reads 0 and
any non-zero is real news.

**Guard against the obvious abuse.** `run_diagnostic`'s doc comment states the
rule as a question about the *number*, not about convenience: reach for `track`
whenever the value would mean something on its own next boot, including when it
has no target — "untargeted is not the same as uncomparable, and conflating the
two is the easier mistake." That conflation is exactly what had happened: the
pass that adopted this rule found **eight** genuine benchmarks printed and
discarded, among them `rdtsc_overhead`, which is the instrument every other
benchmark in the suite is measured with.

**Rejected alternative: infer coverage by name.** The first implementation
diffed the measured names against the scorecard names. It is wrong, and not
marginally: five benchmarks record under a different name than they measure
under (`lock_tracked`→`lock_uncontended`, `heap_raw_alloc_free_64`→
`heap_alloc_free_64`, and three more), so a name diff reports five fully-covered
benchmarks as uncovered. Coverage is now keyed by `BenchResult::seq`, an index
into the measurement list, so a window is covered precisely when *that window*
was handed to `record` — which leaves benchmark naming free rather than making
it load-bearing for an unrelated instrument.

**Where the `track`/`diagnostic` line actually falls: a scaling sweep is
tracked, a decomposition is not.** The first boot under the new instrument
forced this distinction to be stated rather than felt. `bench_pick_next_scaling`
measures run-queue pick cost at five depths; `bench_syscall_dispatch_breakdown`
measures six stages of one syscall. Both are "a group of related numbers", and a
rule of thumb about groups would have put them in the same bucket. The rule that
decides them is instead **what each number claims on its own**:

- A *decomposition*'s stages are meaningless apart from their siblings — stage 3
  of a syscall is not an operation anyone performs, and a comparator handed its
  history has nothing to compare it against. Diagnostics.
- A *scaling sweep*'s points are each a complete measurement of the same
  operation under a different load, and the claim being made is **the shape they
  trace**. Tracked. The in-kernel verdict tests only the two endpoints against a
  4x threshold with generous headroom, so a regression that bent the middle of
  the curve — the exact failure the sweep exists to detect — passed it silently.
  Recording each point is what makes the shape diffable across boots.

That fix needed a rename, not just a wiring change, which is the sharper lesson:
all five depths ran under one name, and five history entries under one key is
not a series but four values overwriting each other. A measurement's name is
part of whether it *can* be recorded at all.

**A second failure of the static scan, in the other direction.** The rejected
alternative above records that a name diff produces false *positives*. The audit
script that replaced it produced a false *negative* on this same sweep: it
searched forward from `run()` for a `score`/`track` naming the same binding and
matched a `&result` belonging to a different function sixty lines downstream.
Two independent static approaches, two opposite errors, one runtime instrument
that got it right — which is the argument for keeping the authority at runtime
rather than adding a third scanner.

**The invariant is asserted, not just printed.** `scripts/boot-test.sh` gained
`check_bench_coverage()`. A coverage line nobody greps is prose, and this project
has a precedent for what prose costs: `BUG-LIVENESS-DEADLINE-FALSE-FIRE` required
a clean liveness log from 2026-07-27 and runs that violated it still exited 0.
The one design point worth recording is the **absent** case — under `--bench` the
line is *required*, because a missing coverage line means the instrument stopped
running, and scoring that as "nothing to complain about" would reproduce this
very bug one level up. It was confirmed to fire against the real pre-fix log
before being trusted on a passing one.

**Where it lives:** `kernel/src/bench.rs` — `MEASUREMENTS`, `note_measurement`,
`declare_diagnostic`, `run_diagnostic`, `record`, and the coverage block in
`print_scorecard`; `scripts/boot-test.sh` — `check_bench_coverage`. Written up in
`known-issues.md` under the scorecard-coverage entry.
