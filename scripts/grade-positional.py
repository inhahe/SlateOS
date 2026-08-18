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
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BENCH_HISTORY = os.path.join(REPO_ROOT, "scripts", "bench-history.py")

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


def grade(bh, canary, positions, lo, hi, clean_tolerance=0):
    """Compare the model's flagged set against a known load window.

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

    result = {
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

    if not flagged:
        result["verdict"] = GRADE_BLIND
        result["reason"] = (
            "the load was applied and the model flagged no benchmark at all"
        )
    elif not clean:
        # Nothing to be wrong about: the widened window swallowed the suite,
        # so this stimulus cannot discriminate no matter what the model does.
        result["verdict"] = GRADE_UNGRADED
        result["reason"] = (
            "the load window, widened by the sampling interval, covers the "
            "whole suite -- there is no clean region to produce a false "
            "positive in, so this run cannot discriminate. Use a narrower "
            "window further from the ends of the suite."
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

    result = grade(bh, canary, positions, lo, hi,
                   clean_tolerance=args.clean_tolerance)
    if result["verdict"] == GRADE_UNGRADED and "covered" not in result:
        print(f"  {GRADE_UNGRADED}: {result['reason']}")
        return 1
    print_grade(result, positions, args.load_at, args.load_until)
    return 0 if result["verdict"] == GRADE_SUPPORTED else 1


if __name__ == "__main__":
    sys.exit(main())
