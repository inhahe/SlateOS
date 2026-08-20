#!/usr/bin/env python3
"""Where the positional contamination model can and cannot see, derived from its code.

# Why this exists

`bench-history.py` converts the benchmark canary's positional trace into a
per-benchmark factor and names the benchmarks that ran while the host was dear
(design-decisions.md §229). RESULT P22 showed it attributes correctly on a real
stimulus. What no run has ever shown is the *shape of its blind spots*, and one
of them is severe enough that a run can pass through the model reporting nothing
while a third of the suite was disturbed.

The blind spot is structural, so it can be derived rather than measured:
`trace_reference` is the **median** of the trace's samples, so once more than
half of them are elevated the disturbance *becomes* the baseline and every
factor lands at 1.0. Sensitivity does not degrade gracefully -- it collapses
from 90% to zero between two adjacent window widths.

That is a consequence of a deliberate design choice, not a defect. The median is
there so a uniformly busy host yields factors of exactly 1.0 and is left to
`global_drift`, the estimator built for that case; a mean would let the burst
drag the baseline toward itself and shrink the correction in proportion to how
badly it was needed. The docstring says so and it is right. What the choice
*also* does -- and what nothing recorded it doing until this script -- is leave
the band between roughly half coverage and full coverage owned by neither
estimator.

# Why it asserts rather than merely printing

The one window width with a measured answer is P22's third run: a 12-benchmark
window carrying one elevated sample, on which the model flagged **7**. This
script rebuilds that run's trace and requires the model to still say 7. If a
change to `interpolate_trace`, `trace_reference` or `POSITIONAL_NOTE_PCT` moves
that number, the graded result in known-issues.md silently stops describing the
code, and the width table quoted there stops being reproducible.

It also pins the 32-wide window that PREDICTION P23 registers its 75% threshold
against. A prediction whose threshold was derived from code that has since
changed is not a prediction; it is a number with a story attached.

# What this is NOT

Not evidence about the machine. Every trace here is synthetic and holds the
elevation perfectly uniform across the window, which no real load does -- the
real one ramps within a sampling interval at each edge. This checks that the
*arithmetic* documented in known-issues.md is the arithmetic the code
implements, nothing more. The physical question is what the P23 run measures.

# Usage

    python scripts/positional-model-limits.py            # table + self-check
    python scripts/positional-model-limits.py --quiet    # self-check only

Exit 0 if the model still behaves as documented, 1 if it does not.
"""
from __future__ import annotations

import argparse
import importlib.util
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def load_bench_history():
    """Import `bench-history.py`, whose hyphen makes it un-importable normally."""
    path = ROOT / "scripts" / "bench-history.py"
    spec = importlib.util.spec_from_file_location("bench_history", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


bh = load_bench_history()

#: Run 3's measured readings, in the integer centicycles `parse_canary_trace`
#: yields. Baseline 5.16 cycles, the one elevated sample 6.17 -- a x1.196
#: excursion, which is what six spinners on a 12-core host produced. Using the
#: real depth matters: the cliff's *position* depends only on the median, but
#: every sensitivity figure in the table depends on how far the excursion clears
#: the 10% flag threshold, and inventing a deeper one would flatter the model.
BASELINE_CENTI = 516
ELEVATED_CENTI = 617

#: Highest occupied suite position in the 86-benchmark suite runs 2 and 3 saw.
LAST_POS = 85


def sample_positions(last_pos=LAST_POS):
    return list(range(0, last_pos + 1, bh.CANARY_SAMPLE_EVERY))


def synthetic_trace(window, last_pos=LAST_POS):
    """The CANARY-TRACE a run whose `window` positions were loaded would print."""
    return [
        {"pos": pos, "centi": ELEVATED_CENTI if pos in window else BASELINE_CENTI}
        for pos in sample_positions(last_pos)
    ]


def score_window(lo, hi, last_pos=LAST_POS):
    """Run the real model over a synthetic window and score it as the grader would."""
    window = set(range(lo, hi + 1))
    positions = {f"b{pos}": pos for pos in range(last_pos + 1)}
    trace = synthetic_trace(window, last_pos)
    factors = bh.positional_factors({"trace": trace}, positions)
    flagged = {
        positions[name]
        for name, factor in factors.items()
        if (factor - 1.0) * 100 >= bh.POSITIONAL_NOTE_PCT
    }
    # The grader's own definition: "within reach" is the window widened by one
    # sampling interval either side, because the model cannot localise finer
    # than its sampling rate. Only the remainder is provably clean, and only
    # that region can distinguish a working model from one that flags freely.
    reach = set(range(lo - bh.CANARY_SAMPLE_EVERY, hi + bh.CANARY_SAMPLE_EVERY + 1))
    clean = set(range(last_pos + 1)) - reach
    return {
        "lo": lo,
        "hi": hi,
        "width": len(window),
        "samples_inside": len([p for p in sample_positions(last_pos) if p in window]),
        "baseline_centi": bh.trace_reference(trace),
        "hits": len(flagged & window),
        "false_positives": len(flagged & clean),
        "clean_size": len(clean),
        "outside_reach": len(flagged - reach),
    }


def fmt(row):
    pct = 100.0 * row["hits"] / row["width"] if row["width"] else 0.0
    return (
        f"  {row['lo']:>2}-{row['hi']:<2}"
        f"  {row['width']:>2} wide"
        f"  {row['samples_inside']} sample(s) inside"
        f"  baseline {row['baseline_centi'] / 100:.2f}"
        f"  sensitivity {row['hits']:>2}/{row['width']:<2} = {pct:5.1f}%"
        f"  false pos {row['false_positives']}/{row['clean_size']}"
        f"  outside reach {row['outside_reach']}"
    )


#: (label, lo, hi, expected hits). Each is quoted in known-issues.md and must
#: keep matching it. `None` means the row is illustrative and not pinned.
PINNED = [
    ("P22 run 3, the only window with a measured answer", 60, 71, 7),
    ("P23's registered window", 32, 63, 28),
]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--quiet", action="store_true",
                        help="run the self-check without printing the table")
    args = parser.parse_args(argv)

    failures = []

    if not args.quiet:
        print("Windows whose result is quoted in known-issues.md:")
    for label, lo, hi, expected in PINNED:
        row = score_window(lo, hi)
        ok = row["hits"] == expected
        if not ok:
            failures.append(
                f"{label}: documented {expected} flagged, model now says {row['hits']}"
            )
        if not args.quiet:
            print(f"{fmt(row)}   {'OK' if ok else 'CHANGED'}  ({label})")

    # The sweep. Anchored at 16 so every width up to 64 still leaves a clean
    # region on both sides; the cliff's position depends on the *count* of
    # elevated samples, not on where the window sits.
    if not args.quiet:
        print("\nWidth sweep -- where the median baseline stops being the "
              "undisturbed reading:")
    cliff = None
    for width in range(bh.CANARY_SAMPLE_EVERY, 65, bh.CANARY_SAMPLE_EVERY):
        row = score_window(16, 16 + width - 1)
        if cliff is None and row["hits"] == 0:
            cliff = row
        if not args.quiet:
            print(fmt(row))

    # The cliff is the finding, so its existence is asserted too. A model that
    # never went blind would be better than the one described -- but the entry
    # in known-issues.md would then be wrong, and a wrong entry about a blind
    # spot is worse than none: it would have people re-running narrow windows
    # to avoid a limit that no longer exists.
    if cliff is None:
        failures.append(
            "the documented blindness cliff no longer occurs at any width up to 64; "
            "known-issues.md's width table and P23's caveat both need revisiting"
        )
    elif not args.quiet:
        print(f"\nBlindness cliff: at {cliff['width']} wide "
              f"({cliff['samples_inside']} of {len(sample_positions())} samples "
              f"elevated) the baseline moves to "
              f"{cliff['baseline_centi'] / 100:.2f} and sensitivity is 0.")
        print("  Above that width the model reports a clean run, which is "
              "indistinguishable\n  from an idle host in every line it prints.")

    if failures:
        print("\nFAILURES -- the code no longer matches what is documented:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    if not args.quiet:
        print("\nModel behaves as known-issues.md and PREDICTION P23 describe.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
