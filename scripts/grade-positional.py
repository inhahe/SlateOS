#!/usr/bin/env python3
"""Grade the positional drift model against a *known* disturbance window.

Run: `python scripts/grade-positional.py --serial build/serial-test.txt \
          --load-at NAME [--load-until NAME]`

Why this exists
---------------
`bench-history.py` grew a positional drift model (design-decisions.md §229):
it interpolates the canary's reference-cost trace across the suite and reports
which benchmarks ran while the host was expensive. That model has never been
shown to predict anything. It was built from one contaminated log, and on that
log it produced a plausible-looking answer -- which is not evidence, because a
model that flags *everything* also produces a plausible-looking answer on a
contaminated log, and a model that flags a fixed arbitrary band produces one
too.

Prediction P22 (known-issues.md) is the experiment that discriminates:
deliberately load the host across a *known interior window* of the suite, then
ask whether the model flags that window and leaves the rest alone. This script
is the grader. `canary-load-test.sh --load-at/--load-until` is the stimulus.

Grading against the raw window would be unfair, and unfair in a way that would
falsely refute the model
--------------------------------------------------------------------------
The canary samples once per `CANARY_SAMPLE_EVERY` benchmarks, and the model
interpolates between samples. So a disturbance confined to positions 41-60
raises whichever samples fall inside it, and the interpolation spreads each
raised sample across the gap to its neighbours on both sides. The narrowest
region the model is *capable* of naming is therefore the window widened by one
sampling interval at each end. Demanding better would be demanding resolution
the instrument does not have -- a limit already documented in §229, not a
defect discovered here.

So the window is graded three ways, and the third is the one that matters:

  covered   the benchmarks that actually ran under load     -> sensitivity
  reachable covered, widened by the sampling interval       -> localisation
  clean     everything else: positions the disturbance      -> false positives
            provably could not have reached, even allowing
            for the instrument's resolution

The false-positive rate over `clean` is the discriminating number. Sensitivity
alone cannot separate a working model from one that flags the whole suite:
"flag everything" scores a perfect 100% on `covered`. Only the clean region
can tell those apart, which is the entire reason the stimulus has to be an
interior window rather than a suffix.

None of that means anything until the stimulus is shown to have landed
------------------------------------------------------------------------
The first run graded FAILED (misplaced), and the verdict was wrong -- not
because the arithmetic was wrong but because the experiment was. Measured
afterwards, the benchmarks in the labelled window had a median slowdown of
**x1.00** while those after it ran **x1.44** slower: the load arrived
somewhere other than where the label said, so the "loaded window" contained
no disturbance for the model to find. The model had in fact flagged the real
disturbance. Grading it against a fictitious one manufactured a refutation.

So this grader now measures the stimulus before it grades the model, from the
benchmark timings themselves -- not from the canary, which is the instrument
under test, and not from the load script, whose claim is the thing that needs
checking. Two conditions must both hold or the verdict is UNGRADED, whatever
the model did:

  * **the load reached the window** -- its benchmarks ran measurably slower
    than the benchmarks *before* it, which are clean by construction (the load
    is triggered by a line the kernel has already printed, so it cannot have
    been applied earlier than the benchmark that triggered it). A window that
    did not slow down was not loaded, and "the model missed it" is then not a
    finding about the model;
  * **the load stayed in the window** -- the benchmarks beyond its reach did
    *not* slow down. Otherwise the region this run scores false positives over
    is not clean, and every flag out there is the model correctly reporting a
    disturbance that the label denies. That is exactly how the first run
    "refuted" the model.

Both use one definition of "this region was disturbed", and that definition
came out of the history rather than out of a principle. The first draft used
the model's own reporting threshold -- 10% on the mean -- which sounded
principled and was useless: across 60 *clean* runs, 38% of benchmarks move 10%
or more on the mean from one run to the next. See `STIMULUS_DISTURBED_MIN` /
`STIMULUS_DISTURBED_MEAN` for the measured null distributions and the 0.4%
false-positive rate the chosen pair carries over 236 clean region-pairs.

The general rule, which cost a whole run to learn: you cannot grade "did the
model find the disturbance" until you have established "there was a
disturbance to find".
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import platform
import statistics
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BENCH_HISTORY = os.path.join(REPO_ROOT, "scripts", "bench-history.py")
DEFAULT_HISTORY = os.path.join(REPO_ROOT, "bench", "history.jsonl")

#: Verdicts. Five rather than pass/fail, because "the model localised the
#: disturbance", "the model saw nothing", "the model flagged the whole suite",
#: "the model flagged the wrong place" and "this run carried no usable trace"
#: are five different findings and only the first supports P22.
GRADE_SUPPORTED = "SUPPORTED"
GRADE_BLIND = "FAILED (blind)"
GRADE_UNLOCALISED = "FAILED (not localised)"
GRADE_MISPLACED = "FAILED (misplaced)"
GRADE_UNGRADED = "UNGRADED"

#: A model that flags most of the suite has not localised anything, however
#: well it covers the loaded window. Above this fraction the localisation
#: claim is vacuous and is reported as such rather than as a pass.
VACUOUS_FLAG_FRACTION = 0.5

#: A region counts as disturbed when **both** its median best-case time and
#: its median mean time are this much worse than the run's undisturbed prefix.
#:
#: These are measured, not chosen, and the measurement is reproducible:
#: `test-grade-positional.py`'s `null_region_rate` replays the 60 clean release
#: runs in `bench/history.jsonl` -- runs with no stimulus at all -- and fails
#: if the numbers below have drifted. A gate whose threshold nobody can
#: re-derive is a gate that quietly rots into a rubber stamp.
#:
#: The first draft used the model's own reporting threshold (10%) on the mean
#: alone, which sounded principled and was useless. Over the 4567
#: benchmark-to-benchmark comparisons in 59 consecutive clean-run pairs:
#:
#:     moved by >= ...        10%    15%    30%
#:     best-case (min)      19.2%  16.1%  10.3%
#:     mean                 38.3%  35.0%  28.6%
#:
#: so a 10%-on-the-mean gate fires on nearly two benchmarks in five with no
#: disturbance present at all.
#:
#: The gate does not operate on single benchmarks, though -- it operates on a
#: *region's median* relative to another region's median, which averages much
#: of that away. Splitting each clean pair's suite into five contiguous blocks
#: and ratioing the last four against the first gives 236 clean region-pairs:
#:
#:     statistic            median   p90    p95    max
#:     best-case ratio       0.998   1.06   1.12   1.61
#:     mean ratio            1.001   1.49   1.68   2.59
#:
#: Both are unbiased (median ~1.0) but the mean is far noisier, because a
#: benchmark's mean is dominated by its worst iterations -- note that neither
#: statistic's p95 clears its own threshold, yet the extremes of both run well
#: past it. That is precisely why both are required: demanding that two
#: statistics with different noise characteristics agree costs **1 false
#: positive in 236** (0.4%), where either alone would cost 5-10x that, against
#: a real disturbance that measured 1.40 best-case and 2.30 mean.
STIMULUS_DISTURBED_MIN = 1.15
STIMULUS_DISTURBED_MEAN = 1.30

#: How the null above is partitioned, and therefore what
#: `test-grade-positional.py` must use to reproduce it. Five blocks: the first
#: stands in for the grader's clean prefix, the other four for regions being
#: tested against it.
STIMULUS_NULL_BLOCKS = 5

#: The measured false-positive rate of the two-metric rule over that null, and
#: the ceiling the regression test holds it to. The gap between the two is
#: deliberate slack for a history that grows: the test exists to catch the
#: suite becoming *much* noisier, not to fail on the next run that shifts one
#: region-pair over a threshold.
STIMULUS_NULL_RATE = 0.004
STIMULUS_NULL_RATE_CEILING = 0.05

#: Benchmarks with a usable baseline needed in each region before the
#: medians mean anything. Four is small, but the point of the floor is to
#: reject "one benchmark in the window had a baseline and it happened to be
#: slow", not to make the medians precise.
STIMULUS_MIN_SAMPLES = 4

#: How many recent clean runs the per-benchmark baseline is taken over.
#:
#: One -- the immediately preceding clean run -- against the instinct that a
#: median over several runs must be less noisy. It is less noisy about the
#: *host* and much noisier about the *kernel*: a window spanning eight runs
#: spans eight commits' worth of performance changes, and those changes are
#: not spread evenly over the suite, so different regions acquire different
#: apparent drifts. Measured on the loaded run, an 8-run baseline put the
#: untouched prefix at x0.70 where the previous run put it at x0.88 -- an
#: error larger than the disturbance being measured. Region-differential code
#: drift beats run-to-run host noise, so the tightest baseline wins.
BASELINE_WINDOW = 1

#: The two per-benchmark statistics the stimulus is measured with, and where
#: each lives in `parse_serial`'s tuple and in a history record.
METRICS = {
    "min": (0, "entries"),
    "mean": (3, "mean_ns"),
}


#: Why each region can be too thin to measure the stimulus with. Each is a
#: separate finding: "the history has no baseline for this suite" is a
#: different problem from "the window was placed too close to the end of the
#: suite", and telling them apart is the difference between fixing the
#: experiment and re-running it unchanged.
STIMULUS_PROBLEMS = {
    "no-baseline": lambda s: (
        "no benchmark in this run has a baseline in the recent clean history, "
        "so the load cannot be located independently of the model that is "
        "being graded against it"),
    "no-pre": lambda s: (
        f"only {s['pre_samples']} benchmark(s) ran before the window, which "
        f"is too few to measure this run's own drift (need "
        f"{STIMULUS_MIN_SAMPLES}). Without that reference a slow window and a "
        f"slow host are indistinguishable. Move the window later in the "
        f"suite."),
    "no-window": lambda s: (
        f"only {s['window_samples']} benchmark(s) in the loaded window have a "
        f"baseline (need {STIMULUS_MIN_SAMPLES}), so there is no measuring "
        f"whether the load reached them"),
    "no-post": lambda s: (
        f"only {s['post_samples']} benchmark(s) ran beyond the disturbance's "
        f"reach (need {STIMULUS_MIN_SAMPLES}), so there is no region in which "
        f"to detect the load escaping its window -- and a load that ran to the "
        f"end of the suite would then be indistinguishable from one confined "
        f"to the window. Move the window earlier in the suite."),
}


def metric_of(entries, metric):
    """{name: value} for one metric, from `parse_serial` output."""
    index = METRICS[metric][0]
    return {name: value[index] for name, value in entries.items()
            if len(value) > index and value[index]}


def baseline_for(bh, records, host, profile, metric, window=BASELINE_WINDOW):
    """Median per-benchmark value over the recent *clean* runs on this host.

    Deliberately built from `comparable_records`, which already excludes
    deliberately-loaded control runs and flagged experiments. Including one
    would put a contaminated run in the baseline and shrink the very slowdown
    this is measuring -- and the runs this grader is used on are precisely the
    ones that get excluded, so the filter is load-bearing rather than
    decorative.
    """
    key = METRICS[metric][1]
    usable = [record for record in bh.comparable_records(records, host, profile)
              if record.get(key)]
    acc = {}
    for record in usable[-window:]:
        for name, value in record[key].items():
            if value and value > 0:
                acc.setdefault(name, []).append(value)
    return {name: statistics.median(vals) for name, vals in acc.items()}


def measure_inflation(current, baseline):
    """{name: this run's value / the clean baseline}, for names in both.

    This is the *ground truth* of the experiment: it says which benchmarks
    actually ran slowly, measured by the benchmarks themselves rather than by
    the canary (which is the instrument under test) or by the script that
    applied the load (which is the claim under test).
    """
    return {
        name: value / base
        for name, value in current.items()
        if value and value > 0 and (base := baseline.get(name)) and base > 0
    }


def stimulus_strength(inflation, positions, lo, hi, reach):
    """Did the load reach the window, and did it stay inside it?

    Measured from the benchmarks themselves, so it is independent of both the
    canary (the instrument under test) and the load script (whose claim is
    what needs checking).

    # Why the reference is the *prefix*, not the whole clean region

    The obvious reference -- "the window ran x1.09, everything provably clean
    ran x0.89, so the stimulus is x1.22" -- is wrong, and wrong in the exact
    way that would have passed the run this check exists to reject. The clean
    region lies on *both* sides of the window, and in that run the load had
    escaped into the tail: the prefix ran x0.88 and the tail x2.31. A median
    over the union of those is a number describing neither, and it happened to
    land low enough to make an unloaded window look stimulated.

    Everything before the window, on the other hand, is clean *by
    construction* and not by assumption. The load is triggered by a line the
    kernel has already printed, so it cannot possibly have been applied before
    the benchmark that triggered it. That makes the prefix's median the run's
    honest baseline drift -- the factor by which this host was fast or slow
    today, with no disturbance in it -- and it is the only region of the run
    about which that can be said without trusting the stimulus.

    # Two regions, one test

    The same question is asked of the window and of the tail, with the same
    definition of "this region was disturbed" (`region_disturbed`):

      landed    the window is disturbed  -- required, or there was nothing
                there for the model to find
      escaped   the tail is *also* disturbed -- disqualifying, because the
                region the run scores false positives over is then not clean,
                and a flag out there would be the model correctly reporting a
                disturbance that the label denies

    Returns a dict; `problem` names a region too thin to speak for, in which
    case neither question was asked.
    """
    def region(predicate, metric):
        return [factor for name, factor in inflation[metric].items()
                if (pos := positions.get(name)) is not None and predicate(pos)]

    before = lambda p: p < lo
    inside = lambda p: lo <= p <= hi
    beyond = lambda p: p > hi + reach

    out = {}
    for metric in METRICS:
        pre = region(before, metric)
        window = region(inside, metric)
        post = region(beyond, metric)
        level = statistics.median(pre) if pre else None
        out[metric] = {
            "level": level,
            "window": (statistics.median(window) / level
                       if window and level else None),
            "post": (statistics.median(post) / level
                     if post and level else None),
            "samples": (len(pre), len(window), len(post)),
        }

    counts = [out[metric]["samples"] for metric in METRICS]
    problem = None
    if not any(inflation.values()):
        problem = "no-baseline"
    elif min(c[0] for c in counts) < STIMULUS_MIN_SAMPLES:
        problem = "no-pre"
    elif min(c[1] for c in counts) < STIMULUS_MIN_SAMPLES:
        problem = "no-window"
    elif min(c[2] for c in counts) < STIMULUS_MIN_SAMPLES:
        problem = "no-post"

    result = {
        "metrics": out,
        "pre_samples": min(c[0] for c in counts),
        "window_samples": min(c[1] for c in counts),
        "post_samples": min(c[2] for c in counts),
        "problem": problem,
        "landed": None,
        "escaped": None,
    }
    if problem is None:
        result["landed"] = region_disturbed(out, "window")
        result["escaped"] = region_disturbed(out, "post")
    return result


def describe_region(stimulus, where):
    """'x1.40 best-case / x2.30 mean' -- both statistics, never just one.

    Quoting one would let a reader draw the conclusion the two-statistic rule
    exists to prevent: the mean alone moves 30% between consecutive clean runs
    often enough that a single number is not evidence of anything.
    """
    parts = []
    for metric, label in (("min", "best-case"), ("mean", "mean")):
        value = stimulus["metrics"][metric][where]
        parts.append(f"{'n/a' if value is None else f'x{value:.2f}'} {label}")
    return " / ".join(parts)


def region_disturbed(metrics, where):
    """True when *both* statistics say this region ran slower than the prefix.

    One definition of "disturbed", asked of the loaded window (where it must
    hold) and of the tail (where it must not). Using one test for both is what
    stops the experiment being graded on a stricter standard for the evidence
    it wants than for the evidence it does not.
    """
    thresholds = {"min": STIMULUS_DISTURBED_MIN, "mean": STIMULUS_DISTURBED_MEAN}
    return all(metrics[metric][where] is not None
               and metrics[metric][where] >= thresholds[metric]
               for metric in METRICS)


def load_bench_history():
    """Import bench-history.py by path (its name is not a valid identifier)."""
    spec = importlib.util.spec_from_file_location("bench_history", BENCH_HISTORY)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {BENCH_HISTORY}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def positions_of(entries):
    """{name: suite position} from `parse_serial` output."""
    return {name: value[6] for name, value in entries.items() if len(value) > 6}


def resolve_window(positions, load_at, load_until):
    """The positions that actually ran under load, as `(lo, hi)` inclusive.

    `lo` is one past `load_at`, not `load_at` itself: the trigger is the line
    the kernel prints when a benchmark *finishes*, so the named benchmark is
    the last one that ran clean, and the first loaded benchmark is the next.
    `hi` is `load_until` itself for the mirror-image reason -- the load is not
    removed until that benchmark has already reported.

    Returns `(lo, hi, error)`; `error` is a message when a name has no SCORE
    line and therefore no position at all.
    """
    if load_at not in positions:
        return None, None, (
            f"'{load_at}' has no SCORE line in this run, so it has no suite "
            f"position. Note that not every benchmark that prints a live line "
            f"is scored (isr_latency scores; isr_hard_irq only prints), and "
            f"only scored ones sit on the canary's axis."
        )
    last = max(positions.values())
    lo = positions[load_at] + 1
    if load_until is None:
        hi = last
    elif load_until not in positions:
        return None, None, (
            f"'{load_until}' has no SCORE line in this run, so it has no "
            f"suite position."
        )
    else:
        hi = positions[load_until]
    if hi < lo:
        return None, None, (
            f"'{load_at}' (position {positions[load_at]}) is not before "
            f"'{load_until}' (position {hi}); the window is empty or reversed."
        )
    return lo, hi, None


def grade(bh, canary, positions, lo, hi, inflation, clean_tolerance=0,
          record_problem=None):
    """Compare the model's flagged set against a known load window.

    `inflation` is `{name: measured slowdown}` from `measure_inflation`, and
    is what turns the load *label* into a load *measurement*. It is a required
    argument rather than an optional refinement on purpose: the first run of
    this experiment produced a confident FAILED against a window that had not
    in fact been loaded, and an optional check is one a caller can forget.
    Pass `{}` to say "no baseline was available", which grades UNGRADED.

    Returns a dict of the counts and the verdict. Pure: no printing, so the
    arithmetic can be tested without capturing stdout.
    """
    factors = bh.positional_factors(canary, positions)
    if not factors:
        return {
            "verdict": GRADE_UNGRADED,
            "reason": "no usable canary trace in this run, so the model "
                      "produced no factors at all",
        }

    reach = bh.CANARY_SAMPLE_EVERY
    last = max(positions.values())
    covered = {p for p in positions.values() if lo <= p <= hi}
    reachable = {p for p in positions.values() if lo - reach <= p <= hi + reach}
    clean = {p for p in positions.values() if p not in reachable}

    flagged_names = {
        name for name, factor in factors.items()
        if (factor - 1.0) * 100 >= bh.POSITIONAL_NOTE_PCT
    }
    flagged = {positions[name] for name in flagged_names if name in positions}

    stimulus = stimulus_strength(inflation, positions, lo, hi, reach)

    result = {
        "stimulus": stimulus,
        "total": len(positions),
        "last": last,
        "window": (lo, hi),
        "reach": reach,
        "reachable_span": (max(0, lo - reach), min(last, hi + reach)),
        "covered": len(covered),
        "clean": len(clean),
        "flagged": len(flagged),
        "hit": len(flagged & covered),
        "localised": len(flagged & reachable),
        "false_positive": len(flagged & clean),
        "outside": sorted(flagged & clean),
        "factors": factors,
    }

    if not clean:
        # Nothing to be wrong about: the widened window swallowed the suite,
        # so this stimulus cannot discriminate no matter what the model does.
        result["verdict"] = GRADE_UNGRADED
        result["reason"] = (
            "the load window, widened by the sampling interval, covers the "
            "whole suite -- there is no clean region to produce a false "
            "positive in, so this run cannot discriminate. Use a narrower "
            "window further from the ends of the suite."
        )
    elif record_problem is not None:
        # Checked before the measured stimulus, because it is a better
        # explanation, not a stronger one. When the controller admits the
        # window had no right-hand edge, "the stimulus never reached the
        # window" is a true statement that sends the reader to look for the
        # wrong fault -- which is what happened on the second attempt, where
        # the printed reason blamed the load's placement and the actual cause
        # was a benchmark name that could never match.
        result["verdict"] = GRADE_UNGRADED
        result["reason"] = (
            f"{LOAD_RECORD_PROBLEMS[record_problem]}. The labelled window is "
            f"therefore not the window that was applied, so this run says "
            f"nothing about the model, whatever the benchmarks show."
        )
    elif stimulus["problem"] is not None:
        # Without a measurable stimulus there is no way to tell a run that was
        # loaded where it says from one that was not, and the difference
        # between those two is the difference between a result and a fiction.
        result["verdict"] = GRADE_UNGRADED
        result["reason"] = STIMULUS_PROBLEMS[stimulus["problem"]](stimulus)
    elif not stimulus["landed"]:
        # The failure that produced the first, bogus FAILED verdict: the
        # labelled window was not slower than the untouched prefix, so it was
        # not loaded, whatever the load script reported.
        result["verdict"] = GRADE_UNGRADED
        result["reason"] = (
            f"the stimulus never reached the window: relative to the "
            f"untouched prefix its benchmarks ran "
            f"{describe_region(stimulus, 'window')} (need at least "
            f"x{STIMULUS_DISTURBED_MIN:.2f} best-case and "
            f"x{STIMULUS_DISTURBED_MEAN:.2f} mean). There was no disturbance "
            f"inside the labelled window for the model to find, so this run "
            f"says nothing about the model."
        )
    elif stimulus["escaped"]:
        # The mirror-image failure, and the one that would silently convict
        # the model: the tail is slowed too, so flags out there are the model
        # being right about a disturbance this run's label denies.
        result["verdict"] = GRADE_UNGRADED
        result["reason"] = (
            f"the stimulus escaped the window: the "
            f"{stimulus['post_samples']} benchmarks beyond the disturbance's "
            f"reach ran {describe_region(stimulus, 'post')} relative to the "
            f"untouched prefix, which is itself a disturbance. The region "
            f"this run would score false positives over is not clean, so a "
            f"false positive there would be the model reporting a real "
            f"disturbance rather than inventing one."
        )
    elif not flagged:
        result["verdict"] = GRADE_BLIND
        result["reason"] = (
            "the loaded window measurably slowed down and the model flagged "
            "no benchmark at all"
        )
    elif len(flagged) > VACUOUS_FLAG_FRACTION * len(positions):
        result["verdict"] = GRADE_UNLOCALISED
        result["reason"] = (
            f"the model flagged {len(flagged)} of {len(positions)} benchmarks; "
            f"covering the loaded window is not evidence when most of the "
            f"suite is covered too"
        )
    elif not flagged & covered:
        result["verdict"] = GRADE_MISPLACED
        result["reason"] = (
            "the model flagged benchmarks, but none of them ran under the load"
        )
    elif result["false_positive"] > clean_tolerance:
        result["verdict"] = GRADE_MISPLACED
        result["reason"] = (
            f"{result['false_positive']} of {len(clean)} benchmarks outside "
            f"the disturbance's reach were flagged (tolerance "
            f"{clean_tolerance})"
        )
    else:
        result["verdict"] = GRADE_SUPPORTED
        result["reason"] = (
            f"{result['hit']} of {len(covered)} loaded benchmarks flagged, "
            f"{result['false_positive']} of {len(clean)} clean ones"
        )
    return result


def print_grade(result, positions, load_at, load_until):
    lo, hi = result["window"]
    rlo, rhi = result["reachable_span"]
    inv = {p: n for n, p in positions.items()}
    print(f"  Load window: positions {lo}-{hi} ({result['covered']} of "
          f"{result['total']} benchmarks), starting after '{load_at}' and "
          f"ending "
          + (f"with '{load_until}'." if load_until else "at the last benchmark."))
    print(f"  Reachable by the model: positions {rlo}-{rhi} -- the window "
          f"widened by the canary's {result['reach']}-benchmark sampling "
          f"interval, which is the narrowest region it can name.")
    # Printed before the model's score, and printed even when the run is
    # ungradeable, because it is the number that says whether the score below
    # is about the model or about the experiment.
    stim = result["stimulus"]
    print("  Stimulus (measured from the benchmarks, independent of both the "
          "model and the load script):")
    drift = " / ".join(
        f"x{stim['metrics'][m]['level']:.2f} {label}"
        if stim["metrics"][m]["level"] is not None else f"n/a {label}"
        for m, label in (("min", "best-case"), ("mean", "mean")))
    print(f"    clean prefix    : {drift} over {stim['pre_samples']} "
          f"benchmarks before the window -- this run's own speed, with no "
          f"disturbance in it, and the reference for the two lines below")
    # `landed`/`escaped` are None when some region was too thin to speak for,
    # and "not disturbed" would then be a claim rather than a report -- the
    # printout above it would read x1.43 / x2.28 and the word next to it would
    # say the opposite. Say which of the three it is.
    def verdict_word(flag, yes, no):
        return "not measured" if flag is None else (yes if flag else no)

    print(f"    loaded window   : {describe_region(stim, 'window')} over "
          f"{stim['window_samples']} benchmarks -- "
          f"{verdict_word(stim['landed'], 'DISTURBED', 'not disturbed')} "
          f"(needs x{STIMULUS_DISTURBED_MIN:.2f} and "
          f"x{STIMULUS_DISTURBED_MEAN:.2f})")
    print(f"    beyond its reach: {describe_region(stim, 'post')} over "
          f"{stim['post_samples']} benchmarks -- "
          f"{verdict_word(stim['escaped'], 'DISTURBED (the load escaped)', 'clean')}")
    print(f"  Model flagged {result['flagged']} of {result['total']} "
          f"benchmarks.")
    print(f"    sensitivity     : {result['hit']} of {result['covered']} "
          f"loaded benchmarks flagged")
    print(f"    localisation    : {result['localised']} of "
          f"{result['flagged']} flagged benchmarks were within reach")
    print(f"    false positives : {result['false_positive']} of "
          f"{result['clean']} provably-clean benchmarks flagged")
    if result["outside"]:
        shown = ", ".join(
            f"{inv.get(p, '?')}@{p}" for p in result["outside"][:8])
        more = len(result["outside"]) - 8
        print(f"      outside reach: {shown}" + (f", +{more} more" if more > 0
                                                 else ""))
    print(f"  VERDICT: {result['verdict']} -- {result['reason']}")


#: What the controller says went wrong, in the grader's words.
#:
#: Accepting these is not a retreat from "the load script is the claim under
#: test". The asymmetry is deliberate and it is the whole point: a record's
#: *confession* that a trigger never matched is admissible, because nothing
#: about the model can follow from a window that was never bounded. Its
#: *claim of success* remains inadmissible, and is still checked against the
#: benchmarks. A defendant's admission is evidence; his denial is not.
LOAD_RECORD_PROBLEMS = {
    "at-never-matched":
        "the load's trigger never matched a live benchmark line, so the load "
        "was never applied",
    "until-never-matched":
        "the load's release never matched a live benchmark line, so the load "
        "ran to the end of the boot and the labelled window has no "
        "right-hand edge",
    # Admissible for the same reason as the other two, and by the same rule:
    # this is the controller reporting, from its spinners' own CPU clocks,
    # that they did not run. That is a confession of an empty window, not a
    # claim to have filled one.
    "load-not-applied":
        "the spinners consumed almost no CPU across the window, so whatever "
        "the benchmarks felt it was not the intended stimulus",
}

#: Mirrors `OCCUPANCY_FLOOR` in `canary-load.py`. Duplicated rather than
#: imported because the grader must be able to read a record produced by a
#: *different* version of the controller -- including one that predates the
#: field, or one whose floor was set differently -- and a grader that adopted
#: its subject's threshold could be talked out of a conviction by the
#: defendant. The number is not load-bearing for good runs: run 3 sat far
#: above it, and the floor only catches spinners that never ran.
OCCUPANCY_FLOOR = 0.5


def print_load_record(path):
    """Report what the load controller says it did.

    Kept strictly separate from the grade above it, and printed after it, to
    keep the distinction that this experiment was built around: the grade is
    measured from the benchmarks, this is the stimulus's own account of
    itself, and the second is never allowed to stand in for the first. It is
    here to *explain* an ungradeable run -- a window that turns out to have
    been 3% of the suite's wall time was never going to perturb a quarter of
    its benchmarks, however precisely it was triggered.
    """
    try:
        with open(path, encoding="utf-8") as handle:
            record = json.load(handle)
    except (OSError, ValueError) as exc:
        print(f"  (no usable load record at {path}: {exc})")
        return
    print("  Load controller's own account (diagnostic, not evidence):")
    print(f"    applied         : {record.get('fired')} "
          f"/ released: {record.get('released')} "
          f"/ outcome: {record.get('outcome')}")
    if record.get("load_seconds") is not None:
        line = f"    held for        : {record['load_seconds']:.2f}s"
        if record.get("suite_seconds_seen"):
            share = 100 * record["load_seconds"] / record["suite_seconds_seen"]
            line += (f" of a {record['suite_seconds_seen']:.2f}s suite "
                     f"({share:.1f}%)")
        print(line)
    print(f"    result lines    : {record.get('completions_during')} under "
          f"load, after {record.get('completions_before_on')} clean")
    # What the spinners actually did, from their own CPU clocks. This is the
    # only line in this block that can distinguish "the load was switched on"
    # from "the load actually ran" -- the distinction the first two attempts
    # conflated, and one the host canary below turned out to be incapable of
    # making at all.
    occupancy = record.get("host_occupancy") or {}
    if occupancy.get("occupancy") is not None:
        print(f"    spinner CPU     : {occupancy['occupancy'] * 100:.0f}% "
              f"occupancy ({occupancy['cpu_seconds_total']:.2f}s burned of "
              f"{occupancy['expected_cpu_seconds']:.2f}s available across "
              f"{occupancy['spinners']} spinner(s))")

    host = record.get("host_probe") or {}
    if host.get("inflation") is not None:
        # Records written before RESULT P22 carry the *median* ratio in
        # `inflation`; newer ones carry the best-case. They are told apart by
        # the presence of `median_inflation`, and labelled accordingly rather
        # than silently compared -- run 3's median read x0.78 ("the host got
        # faster under load") where its best-case read x1.02, and only the
        # second of those is a measurement.
        legacy = "median_inflation" not in host
        stat = "median, unreliable" if legacy else "best-case"
        print(f"    host canary     : x{host['inflation']:.2f} ({stat}) over "
              f"{host['samples']} samples "
              f"(the host's own cost of a fixed unit of work, "
              f"loaded vs idle)")
        if not legacy and host.get("median_inflation") is not None:
            print(f"                      median x{host['median_inflation']:.2f}"
                  f" -- retained for comparison only; its run-to-run spread on"
                  f" this host exceeds the effect it measures")
        print("                      near x1.00 is expected when the host has"
              " more cores than spinners, and is not evidence either way")
    problem = record_problem(record)
    if problem:
        print(f"    PROBLEM         : {LOAD_RECORD_PROBLEMS.get(problem, problem)}")


def record_problem(record):
    """Read the confession out of a record, deriving it if it is not stated.

    The `problem` field is the controller's own summary, but the grader does
    not depend on it. The two facts it summarises -- a `--at` that never
    fired, a `--until` that never released -- are already in the record as
    plain booleans, and deriving from those keeps this working on records
    written before the field existed and on ones written by a controller
    whose version we cannot see. A grader that can only detect the faults its
    subject volunteers is not much of a grader.

    A missing `until` is not a fault: an open-ended window is a legitimate
    thing to ask for, and only a *labelled* right-hand edge that never
    arrived makes the applied window differ from the label.
    """
    stated = record.get("problem")
    if stated in LOAD_RECORD_PROBLEMS:
        return stated
    if record.get("at") and record.get("fired") is False:
        return "at-never-matched"
    if record.get("until") and record.get("released") is False:
        return "until-never-matched"
    occupancy = record.get("host_occupancy") or {}
    if occupancy.get("occupancy") is not None \
            and occupancy["occupancy"] < OCCUPANCY_FLOOR:
        return "load-not-applied"
    return None


def load_record_problem(path):
    """The controller's own admission that the window is not the labelled one.

    Returns None when there is no record, when it cannot be read, or when it
    reports no problem -- absence of a confession is never taken as evidence
    of innocence, which is what the measured stimulus gate is for.
    """
    if not path:
        return None
    try:
        with open(path, encoding="utf-8") as handle:
            record = json.load(handle)
    except (OSError, ValueError):
        return None
    if not isinstance(record, dict):
        return None
    return record_problem(record)


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Grade the positional drift model against a known load "
                    "window (prediction P22).")
    parser.add_argument("--serial", default=os.path.join(
        REPO_ROOT, "build", "serial-test.txt"),
        help="serial log of the loaded run (default: build/serial-test.txt)")
    parser.add_argument("--load-at", required=True,
                        help="benchmark after which the load was applied")
    parser.add_argument("--load-until", default=None,
                        help="benchmark at which the load was removed "
                             "(default: it ran to the end of the suite)")
    parser.add_argument("--positions-only", action="store_true",
                        help="just resolve the names to suite positions and "
                             "exit; used as a pre-flight against the previous "
                             "run's log")
    parser.add_argument("--label", default=None,
                        help="prefix for the --positions-only line")
    parser.add_argument("--clean-tolerance", type=int, default=0,
                        help="how many provably-clean benchmarks may be "
                             "flagged before the run counts as misplaced "
                             "(default 0)")
    parser.add_argument("--history", default=DEFAULT_HISTORY,
                        help="benchmark history the per-benchmark baseline "
                             "is taken from (default: bench/history.jsonl)")
    parser.add_argument("--host", default=None,
                        help="host whose records form the baseline "
                             "(default: this machine)")
    parser.add_argument("--profile", default="release",
                        help="build profile the baseline is taken from; "
                             "--bench runs are release (default: release)")
    parser.add_argument("--load-record", default=None,
                        help="JSON record written by canary-load.py; its "
                             "timings are reported alongside the grade as "
                             "diagnostics for a window that did not land")
    args = parser.parse_args(argv)

    bh = load_bench_history()
    entries = bh.parse_serial(args.serial)
    if not entries:
        print(f"grade-positional: no SCORE lines in {args.serial}",
              file=sys.stderr)
        return 2
    positions = positions_of(entries)
    if not positions:
        print(f"grade-positional: {args.serial} predates suite positions",
              file=sys.stderr)
        return 2

    lo, hi, error = resolve_window(positions, args.load_at, args.load_until)
    if error:
        print(f"grade-positional: {error}", file=sys.stderr)
        return 2

    if args.positions_only:
        prefix = f"    {args.label}: " if args.label else "    "
        # "of 0-85", not "of 85": the suite's last *index* is 85 while its
        # *count* is 86, and this tool exists precisely to stop that off-by-one
        # being carried into an experiment's ground truth.
        print(f"{prefix}positions {lo}-{hi} of 0-{max(positions.values())} "
              f"({hi - lo + 1} of {len(positions)} benchmarks under load)")
        return 0

    canary = bh.parse_canary(args.serial)
    if canary is None:
        print("  UNGRADED: the run recorded no canary at all, so "
              "contamination is unknown rather than measured.")
        return 1

    host = args.host or platform.node() or "unknown"
    records = bh.load_history(args.history)
    inflation = {
        metric: measure_inflation(
            metric_of(entries, metric),
            baseline_for(bh, records, host, args.profile, metric))
        for metric in METRICS
    }

    result = grade(bh, canary, positions, lo, hi, inflation,
                   clean_tolerance=args.clean_tolerance,
                   record_problem=load_record_problem(args.load_record))
    if result["verdict"] == GRADE_UNGRADED and "covered" not in result:
        print(f"  {GRADE_UNGRADED}: {result['reason']}")
        if args.load_record:
            # Printed even on the early return, because when the verdict is
            # UNGRADED *because* of the record, suppressing the record leaves
            # the reader with an accusation and no exhibit.
            print_load_record(args.load_record)
        return 1
    print_grade(result, positions, args.load_at, args.load_until)
    if args.load_record:
        print_load_record(args.load_record)
    return 0 if result["verdict"] == GRADE_SUPPORTED else 1


if __name__ == "__main__":
    sys.exit(main())
