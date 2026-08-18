#!/usr/bin/env python3
"""Regression tests for `scripts/grade-positional.py`.

Run: `python scripts/test-grade-positional.py` (exit 0 = pass, 1 = fail).
No pytest dependency, matching `test-bench-history.py` next door.

What is and is not tested here
------------------------------
This file tests the **grader**, not the model. The distinction matters enough
to state plainly, because conflating them is how a project ends up believing a
prediction was confirmed by its own test suite:

  * The grader is graded here in full, with a stub model, and specifically for
    its ability to *fail*. A grader that cannot return FAILED is worth nothing
    -- by the maxim this project keeps relearning, a check that cannot fire is
    indistinguishable from a check that passes -- so every one of the five
    verdicts has a test that produces it.

  * The real model is exercised end-to-end on a *synthetic* serial log whose
    trace was written by hand to be exactly the shape a localised disturbance
    would produce. That test proves the pieces are wired together and that the
    arithmetic runs; it proves nothing about real hosts. It is a smoke test,
    not evidence.

Prediction P22 is graded by running `canary-load-test.sh --load-at/--load-until`
on real hardware against a real disturbance. Nothing in this file substitutes
for that, and a green run here must never be reported as P22 confirmed.
"""

from __future__ import annotations

import contextlib
import importlib.util
import inspect
import io
import os
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "grade-positional.py")

_FAILURES = []


def load_module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"      got:  {got!r}")
    print(f"      want: {want!r}")
    _FAILURES.append(label)
    return False


def check_true(label, got):
    return check(label, bool(got), True)


class StubModel:
    """A model whose flagged set is dictated, not computed.

    The grader's failure paths exist for models that behave badly, and no
    trace can make the *real* model flag the entire suite -- its baseline is
    the median of its own samples, so a uniformly raised trace yields a factor
    of 1.0 everywhere (design-decisions.md §229). That property is a virtue of
    the model and an obstacle to testing the grader, so the grader is fed a
    stub that can misbehave in ways the real model structurally cannot.
    """

    CANARY_SAMPLE_EVERY = 8
    POSITIONAL_NOTE_PCT = 10.0

    def __init__(self, flagged, empty=False):
        self.flagged = set(flagged)
        self.empty = empty

    def positional_factors(self, canary, positions):
        if self.empty:
            return {}
        return {name: (1.5 if pos in self.flagged else 1.0)
                for name, pos in positions.items()}


def synthetic_positions(count=86):
    return {f"b{i:02d}": i for i in range(count)}


# --- the window arithmetic ------------------------------------------------

def test_window_starts_after_the_named_benchmark_not_at_it():
    """The trigger line means "finished", so the named benchmark ran clean.

    Off by one here would be invisible and corrosive: it would shift the whole
    ground truth one position against the model being graded, which is the one
    quantity in this experiment that must not be an estimate.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    lo, hi, err = gp.resolve_window(positions, "b40", "b60")
    check("window starts one past --load-at", (lo, err), (41, None))
    check("window ends at --load-until itself", hi, 60)


def test_window_without_an_end_runs_to_the_last_benchmark():
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    lo, hi, err = gp.resolve_window(positions, "b40", None)
    check("open-ended window reaches the last position", (lo, hi, err),
          (41, 85, None))


def test_unscored_names_are_refused_rather_than_guessed():
    """A live-printed name with no SCORE line has no position at all.

    `isr_hard_irq` prints a result line and `isr_latency` is what gets scored,
    so a plausible-looking name can genuinely be absent from the axis. Guessing
    a position for it -- say, the nearest scored neighbour -- would fabricate
    the experiment's ground truth.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    _, _, err = gp.resolve_window(positions, "isr_hard_irq", None)
    check_true("unscored --load-at is an error", err and "no SCORE line" in err)
    _, _, err = gp.resolve_window(positions, "b10", "not_a_benchmark")
    check_true("unscored --load-until is an error",
               err and "no SCORE line" in err)


def test_reversed_window_is_refused():
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    _, _, err = gp.resolve_window(positions, "b60", "b40")
    check_true("reversed window is an error", err and "reversed" in err)


# --- the grader's verdicts, all five reachable ----------------------------

def test_grader_supports_a_model_that_flags_the_window_and_nothing_else():
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    model = StubModel(range(41, 61))
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60)
    check("exact hit is SUPPORTED", result["verdict"], gp.GRADE_SUPPORTED)
    check("sensitivity counted", result["hit"], 20)
    check("no false positives", result["false_positive"], 0)


def test_grader_allows_the_sampling_intervals_spill():
    """Flagging into the widened band must not count against the model.

    The canary samples once per 8 benchmarks and the model interpolates
    between samples, so a disturbance at 41-60 necessarily smears across the
    gaps to its neighbouring samples. Penalising that would be penalising the
    instrument's documented resolution, and would refute the model for being
    honest about it.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    model = StubModel(range(33, 69))          # exactly the reachable band
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60)
    check("spill within one sampling interval is SUPPORTED",
          result["verdict"], gp.GRADE_SUPPORTED)
    check("spill produces no false positives", result["false_positive"], 0)


def test_grader_fails_a_model_that_flags_nothing():
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    model = StubModel([])
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60)
    check("silence under a known load is BLIND",
          result["verdict"], gp.GRADE_BLIND)


def test_grader_fails_a_model_that_flags_the_whole_suite():
    """The failure mode the interior window exists to catch.

    A model that flags everything scores 100% sensitivity on any stimulus. If
    the grader called that SUPPORTED, the whole experiment would be incapable
    of a negative result.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    model = StubModel(range(86))
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60)
    check("flagging everything is NOT LOCALISED",
          result["verdict"], gp.GRADE_UNLOCALISED)
    check("perfect sensitivity did not save it", result["hit"], 20)


def test_grader_fails_a_model_that_flags_the_wrong_place():
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    model = StubModel(range(0, 20))
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60)
    check("flagging elsewhere is MISPLACED",
          result["verdict"], gp.GRADE_MISPLACED)
    check("the offending positions are reported",
          result["outside"][:3], [0, 1, 2])


def test_grader_fails_a_model_that_mostly_hits_but_leaks():
    """One flagged benchmark outside the disturbance's reach is a finding.

    Default tolerance is zero deliberately: this is an experiment run a
    handful of times, not a CI gate, and a leak that gets waved through is a
    leak nobody investigates.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    model = StubModel(list(range(41, 61)) + [5])
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60)
    check("a single leak is MISPLACED", result["verdict"],
          gp.GRADE_MISPLACED)
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60,
                      clean_tolerance=1)
    check("...unless the tolerance is raised explicitly",
          result["verdict"], gp.GRADE_SUPPORTED)


def test_grader_refuses_a_stimulus_that_cannot_discriminate():
    """A window whose widened band swallows the suite proves nothing.

    Reported as UNGRADED rather than SUPPORTED, because with no clean region
    there is nowhere for a false positive to appear -- so the run would pass
    whatever the model did.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions(20)
    model = StubModel(range(20))
    result = gp.grade(model, {"trace": "ignored"}, positions, 5, 14)
    check("no clean region means UNGRADED",
          result["verdict"], gp.GRADE_UNGRADED)


def test_grader_reports_a_missing_trace_as_ungraded_not_as_clean():
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    result = gp.grade(StubModel([], empty=True), {}, positions, 41, 60)
    check("no factors means UNGRADED", result["verdict"], gp.GRADE_UNGRADED)
    check("and no counts are invented", "covered" in result, False)


# --- end to end, on the real model, over a synthetic log ------------------

def _synthetic_serial(count=86, elevated=(48, 56), base="5.00", high="15.00"):
    """A serial log shaped like a run disturbed only across one window."""
    lines = []
    for i in range(count):
        lines.append(f"[bench] SCORE b{i:02d} {100 + i} - TRACK {200 + i} 500 0")
    samples = []
    for pos in range(0, count, 8):
        samples.append(f"{pos}:{high if pos in elevated else base}")
    samples.append(f"start:{base}")
    samples.append(f"end:{base}")
    # start end pct min max spread samples invalid min_centi max_centi
    canary = "[bench] CANARY 5 5 100 5 15 200 13 0 500 1500"
    trace = "[bench] CANARY-TRACE " + " ".join(samples)
    return "\n".join(lines + [canary, trace]) + "\n"


def test_end_to_end_on_a_synthetic_localised_disturbance():
    """Smoke test of the whole chain: log -> positions -> model -> verdict.

    Synthetic, and therefore NOT evidence for P22 -- the trace here was
    written to be the right answer. What it does establish is that
    `parse_serial`'s positions, `parse_canary`'s trace and the interpolation
    actually join up, which is the part most likely to break silently when any
    of the three changes.
    """
    gp = load_module(SCRIPT, "grade_positional")
    handle, path = tempfile.mkstemp(suffix=".txt")
    try:
        with os.fdopen(handle, "w") as out:
            out.write(_synthetic_serial())
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = gp.main(["--serial", path, "--load-at", "b40",
                          "--load-until", "b60"])
        text = buf.getvalue()
        check("localised synthetic disturbance grades SUPPORTED and exits 0",
              rc, 0)
        check_true("the verdict is printed", gp.GRADE_SUPPORTED in text)
        check_true("the window is stated", "positions 41-60" in text)
        check_true("the resolution limit is stated", "widened" in text)
    finally:
        os.unlink(path)


def test_end_to_end_flat_trace_is_not_reported_as_a_hit():
    """An undisturbed run must not grade as SUPPORTED just because load ran.

    The negative control for the test above. If a flat trace produced a
    SUPPORTED verdict, every run would confirm P22 regardless of what
    happened, which is the failure the whole file is written against.
    """
    gp = load_module(SCRIPT, "grade_positional")
    handle, path = tempfile.mkstemp(suffix=".txt")
    try:
        with os.fdopen(handle, "w") as out:
            out.write(_synthetic_serial(elevated=()))
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = gp.main(["--serial", path, "--load-at", "b40",
                          "--load-until", "b60"])
        check("a flat trace does not grade SUPPORTED", rc, 1)
        check_true("and says the model saw nothing",
                   gp.GRADE_BLIND in buf.getvalue())
    finally:
        os.unlink(path)


def test_positions_only_mode_reports_the_window_and_nothing_else():
    gp = load_module(SCRIPT, "grade_positional")
    handle, path = tempfile.mkstemp(suffix=".txt")
    try:
        with os.fdopen(handle, "w") as out:
            out.write(_synthetic_serial())
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = gp.main(["--serial", path, "--load-at", "b40",
                          "--load-until", "b60", "--positions-only",
                          "--label", "predicted"])
        text = buf.getvalue()
        check("--positions-only exits 0", rc, 0)
        check_true("reports the window", "positions 41-60" in text)
        check_true("carries the label", "predicted" in text)
        check_true("does not grade", gp.GRADE_SUPPORTED not in text)
    finally:
        os.unlink(path)


def main():
    tests = [obj for name, obj in sorted(globals().items())
             if name.startswith("test_") and inspect.isfunction(obj)]
    for test in tests:
        test()
    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILURE(S): " + ", ".join(_FAILURES))
        return 1
    print(f"All {len(tests)} test group(s) passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
