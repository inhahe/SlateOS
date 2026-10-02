### [A] B-BENCH-CONFIRMED-REGRESSIONS-FIRE-ON-AN-UNCHANGED-BINARY-EVEN-ON-A-CLEAN-RUN — 2026-08-16 — ✅ FIXED 2026-08-16 (`scripts/bench-history.py`, replication gate)

**Status:** FIXED. Follows on from
`### B-BENCH-COMPARES-TO-ONE-PRIOR-RUN-NOT-THE-DISTRIBUTION`, whose
2026-08-15 fix added each benchmark's **own recent range** as a second
gate precisely so a verdict could not be made from a single run-over-run
delta. That fix helped and is not being undone here. What is new is a
measurement of what it still lets through, and the number is not small.

> **Corrected 2026-08-16, same day.** As first committed (`734c77a32`)
> this entry asserted the gate used "an 8-run min-max" and recommended
> replacing it with an IQR/MAD band. Both were wrong: the gate has been
> Tukey's fence over quartiles since 2026-08-15, so the recommendation
> was to install what was already installed. The error came from writing
> the fix up from the *report's* wording ("its own range is 293-453ns")
> without reading `per_benchmark_bands()`. Point 2 below now carries the
> measurement that replaced the guess, and it reverses the conclusion —
> band tuning cannot fix this at all.

**What was measured.** Two `./scripts/boot-test.sh --bench` runs of the
**same commit** (`602fc62e0`), 2.5 minutes apart, nothing rebuilt between
them — an A/A test, where by construction every reported regression is a
false positive:

| | run A `10:39:57` | run B `10:42:35` |
|---|---|---|
| harness run verdict | `RUN CONTAMINATED` | **`RUN CLEAN`** |
| `REGRESSED` (confirmed) | `pick_next` +92% | `page_alloc_free` +85%, `vfs_stat_breakdown_full` +36% |
| `REGRESSED, UNCONFIRMED` | `futex_wait_mismatch` +38% | `heap_raw_alloc_free_512` +54% |

Drift-corrected, **5 of 83 benchmarks moved >25% between two runs of one
binary**. The distribution is not uniformly noisy — it is a thin tail on
a stable body:

    |change| across 83 benchmarks:  median 2%   p90 9%   max 85%

**The two findings that matter, neither of which is "benchmarks are noisy".**

1. **`RUN CLEAN` does not mean the verdicts are trustworthy.** Run B
   passed every contamination instrument — canary steady, dispersion 16
   within the host band of 16.0, wall time within band — and still
   produced two *confirmed* regressions on code that had not changed.
   The run-level verdict and the per-benchmark verdict are answering
   different questions, and the output currently invites reading the
   first as a warrant for the second.
2. **The band is not the weak link, and cannot be made into the fix.**
   The gate is already robust — `per_benchmark_bands()` takes Tukey's
   fence (`TUKEY_K = 1.5`) over the trailing `SPEED_WINDOW = 8`
   *comparable* runs, not a min-max. It flagged these values because
   they genuinely are far outside where those benchmarks sit:
   `page_alloc_free` fence 293-453ns, observed 680ns; `pick_next` fence
   415-810ns, observed 1052ns. **The band is right and the conclusion is
   still wrong**, because the outlier is environmental, not textual.

   Replaying the real history through the module's own
   `comparable_records()` / `per_benchmark_bands()` — importing them
   rather than reimplementing, since a hand-rolled replay produced
   fences nowhere near the printed ones (the window filters by host and
   profile, and this history mixes profiles) — shows that widening the
   window does not help and mostly hurts:

   | window | k=1.5 fence, `page_alloc_free` | verdict on 680ns |
   |---|---|---|
   | 8 (current) | 293-453 | promoted |
   | 12 | 152-689 | declined |
   | 16 | 293-456 | promoted |
   | 27 (all comparable) | 309-427 | promoted, **narrower** |

   More history makes the fence *tighter*, because these tail events are
   rare enough that they never move the quartiles. Loosening `k` to 3.0
   declines `pick_next` only at window ≥ 24 and never declines
   `page_alloc_free` at all — and a `k` wide enough to swallow an 85%
   excursion is wide enough to swallow the 2x regression the suite
   exists to catch. **No (window, k) setting separates these false
   positives from real regressions**, which is the finding: on the
   evidence available to the band, the two are not different.

**Why `pick_next` is the instructive case.** In run A it was the single
confirmed regression, +92%, on the scheduler path `CLAUDE.md` lists as
performance-critical and requires to stay O(1). Normalised to the suite
median it read 753/1000 against a 34-run historical range of 271-534 —
i.e. an outlier by the *history*, not just by the previous run. It was
still noise: run B put it at 1177ns and did not flag it at all. A
plausible-looking regression, on a benchmark that matters, corroborated
by a second statistic, was wrong.

**What this does *not* say.** It is not evidence that the suite is
useless: 90% of benchmarks agreed to within 9% across the pair, which is
enough to catch the kind of regression that matters (a 2x algorithmic
loss). The problem is confined to how a *verdict* is derived from one
comparison.

**What the proper fix looks like.** Do not call anything `REGRESSED`
from a single pair of runs. The harness already records history and
already computes a whole-suite median, so the material is present:

- **Require replication — this is the whole fix.** Promote to
  `REGRESSED` only when the movement survives a second run of the *same
  commit*. This is the only mechanism that discriminates, and the reason
  is structural rather than statistical: a code-caused regression
  reproduces on the same binary, an environment-caused outlier does not.
  Nothing computable from a single run can tell them apart, which is
  precisely what the (window, k) sweep above demonstrates. The A/A pair
  shows the cost is one extra boot — ~2 min, no rebuild, since the
  binary is already staged — against a false positive's cost in reader
  attention.
- **Record each benchmark's own A/A noise floor** and refuse to judge
  any movement smaller than it. `page_alloc_free` demonstrably has a
  floor near 85%; a 30% move in it is unjudgeable and should say so, the
  way `P21(b)` was correctly recorded as `UNGRADEABLE` rather than
  graded. This needs repeated same-commit runs to populate, so it falls
  out of the replication work rather than being extra effort.
- **Do not touch `TUKEY_K` or `SPEED_WINDOW`.** Measured above: the
  sweep changes *which* benchmarks are falsely promoted without reducing
  how many, and the loosening that would decline these two also declines
  real regressions. Tuning them would buy a quieter report and a blinder
  one.

**Where it lives:** `scripts/bench-history.py` (verdict logic),
`scripts/boot-test.sh --bench` (invocation), `bench/history.jsonl` (the
34-run record the A/A above was drawn from; the last two entries share
commit `602fc62e0` and are the pair in question).

**What was done (2026-08-16).** Both bullets above, and one more that the
replay made obvious. `scripts/bench-history.py` gained:

1. **A replication gate** (`replication_verdict`, `values_for_commit`).
   Every movement that crossed the threshold *and* left its band is now
   asked one further question: did this same commit produce it more than
   once? Three outcomes, and the asymmetry between the last two is the
   design. `REPLICATED` — every recorded run of the commit shows it —
   keeps the word `REGRESSED`. `CONTRADICTED` — another run of the same
   binary landed back inside the range — is withdrawn under a
   `NOT REPLICATED` heading and does **not** fail the build.
   `UNREPLICATED` — the commit was measured once — prints as
   `REGRESSED, UNREPLICATED` and **still fails**, because excusing it
   would silence the check in the ordinary case (one run per commit is
   the norm), and that is the same failure as a check that cannot fire.
   Only a positively-evidenced contradiction withdraws a claim, exactly
   the standard `MODE_UNDECIDED` is already held to.
2. **The A/A comparison is named outright.** If the baseline record and
   the current run share a commit, the whole run-over-run diff is an A/A
   test and no movement in it can have been caused by code — arithmetic,
   not statistics, since the difference has no code term. The report says
   so above every list and the run-over-run claims stop failing the
   build. Sustained shifts are deliberately *not* excused: `level_shifts`
   measures against a baseline drawn from earlier commits, so that
   comparison is not self-referential. This is what the entry above was
   missing — it covers the movements with too little history for a band,
   which the per-benchmark gate declines to judge and which would
   otherwise still have printed as `REGRESSED, UNCONFIRMED` on a binary
   compared with itself.
3. **The per-benchmark A/A noise floor is printed** wherever repeats
   exist, which is the second bullet above falling out of the first as
   predicted: `same-commit runs: [363, 680] -- a 87% spread with no code
   change`.

`TUKEY_K` and `SPEED_WINDOW` were not touched, per the third bullet.

**Verified against the data that caused it, not only synthetically.**
`test_replication_declines_the_measured_false_positives` replays run B
against run A out of the real `bench/history.jsonl`: `report()` returned
True before and returns False now, `page_alloc_free` and
`vfs_stat_breakdown_full` are both withdrawn by name, and no benchmark
gets the confirmed heading. Six new tests, 43 assertions. Both directions
were mutation-tested — forcing the verdict to `REPLICATED` fails 14
assertions, forcing it to `CONTRADICTED` fails 5 (including "a
replicated movement fails the build") — because a gate that only ever
withdraws is indistinguishable from deleting the check, and a test that
cannot fail is the thing this whole file exists to catch.

**What deliberately remains.** A first, single run of a genuinely quiet
commit that hits a tail outlier still fails the build as
`REGRESSED, UNREPLICATED`, and that is the intended trade: the report now
tells the reader it is one run and names the exact command that settles
it (`./scripts/boot-test.sh --bench` again, no rebuild, ~2 min). The
alternative — treating unmeasured as absolved — buys a quiet report by
making the check unable to fire.

**Standing lesson this is another instance of.** Conservation is not
placement, and a clean instrument is not a correct conclusion: run B's
contamination checks all passed and its regression list was still wrong.
A check that says something false is worse than no check, because it is
believed — and the cost lands on the *next* real regression, which gets
waved through as "probably noise again".
