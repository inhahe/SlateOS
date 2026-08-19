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

  * One test is not synthetic at all: `test_the_stimulus_gate_is_still_above
    _the_noise_floor` re-derives the gate's false-positive rate from the real
    `bench/history.jsonl`. The gate's thresholds were set from that
    measurement, and a threshold justified by a measurement nobody re-runs is
    a threshold that decays into a rubber stamp as the suite gets noisier.

Prediction P22 is graded by running `canary-load-test.sh --load-at/--load-until`
on real hardware against a real disturbance. Nothing in this file substitutes
for that, and a green run here must never be reported as P22 confirmed.
"""

from __future__ import annotations

import contextlib
import importlib.util
import inspect
import io
import json
import os
import platform
import statistics
import sys
import tempfile

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO_ROOT, "scripts", "grade-positional.py")
BENCH_HISTORY = os.path.join(REPO_ROOT, "scripts", "bench-history.py")
REAL_HISTORY = os.path.join(REPO_ROOT, "bench", "history.jsonl")

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


#: `StubModel.CANARY_SAMPLE_EVERY`, and the real model's value too. The
#: stimulus helper needs it because the band between the window and `hi+reach`
#: is measured by neither region, and a helper that guessed differently from
#: the grader would silently shift which benchmarks land in "post".
REACH = 8


def landed_stimulus(positions, lo, hi, reach=REACH,
                    window=(1.40, 2.00), post=(1.00, 1.00), omit=None):
    """Measured inflation describing a load that landed in `(lo, hi)` and stayed.

    This is the argument the grader will not run without, and the reason it
    exists is worth restating at the point where the tests fake it: `grade`
    takes the *measured* slowdown rather than the load script's claim, because
    the first real run of this experiment produced a confident FAILED against
    a window that had never been loaded. Every verdict test below therefore
    has to state, explicitly, that its hypothetical run had a real disturbance
    in the labelled place -- which is exactly the discipline the grader is
    meant to impose on its callers.

    Defaults are the shape of a genuine hit: flat before the window, x1.40
    best-case and x2.00 mean inside it, flat again beyond its reach. `omit` is
    a predicate on position for benchmarks with no baseline at all, used to
    starve one region and provoke the corresponding `STIMULUS_PROBLEMS` entry.
    """
    levels = {"min": (1.0, window[0], post[0]),
              "mean": (1.0, window[1], post[1])}
    out = {}
    for metric, (pre, mid, tail) in levels.items():
        values = {}
        for name, pos in positions.items():
            if omit is not None and omit(pos):
                continue
            if pos < lo:
                values[name] = pre
            elif pos <= hi + reach:
                # Inside the window, or in the widened band the grader
                # measures with neither region.
                values[name] = mid
            else:
                values[name] = tail
        out[metric] = values
    return out


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
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60,
                      landed_stimulus(positions, 41, 60))
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
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60,
                      landed_stimulus(positions, 41, 60))
    check("spill within one sampling interval is SUPPORTED",
          result["verdict"], gp.GRADE_SUPPORTED)
    check("spill produces no false positives", result["false_positive"], 0)


def test_grader_fails_a_model_that_flags_nothing():
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    model = StubModel([])
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60,
                      landed_stimulus(positions, 41, 60))
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
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60,
                      landed_stimulus(positions, 41, 60))
    check("flagging everything is NOT LOCALISED",
          result["verdict"], gp.GRADE_UNLOCALISED)
    check("perfect sensitivity did not save it", result["hit"], 20)


def test_grader_fails_a_model_that_flags_the_wrong_place():
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    model = StubModel(range(0, 20))
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60,
                      landed_stimulus(positions, 41, 60))
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
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60,
                      landed_stimulus(positions, 41, 60))
    check("a single leak is MISPLACED", result["verdict"],
          gp.GRADE_MISPLACED)
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60,
                      landed_stimulus(positions, 41, 60), clean_tolerance=1)
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
    result = gp.grade(model, {"trace": "ignored"}, positions, 5, 14,
                      landed_stimulus(positions, 5, 14))
    check("no clean region means UNGRADED",
          result["verdict"], gp.GRADE_UNGRADED)


def test_grader_reports_a_missing_trace_as_ungraded_not_as_clean():
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    result = gp.grade(StubModel([], empty=True), {}, positions, 41, 60, {})
    check("no factors means UNGRADED", result["verdict"], gp.GRADE_UNGRADED)
    check("and no counts are invented", "covered" in result, False)


# --- the stimulus gate: was there a disturbance to find at all? -----------

def test_grader_refuses_a_window_the_load_never_reached():
    """The regression test for the run that started all of this.

    P22's first run labelled positions 41-73 as loaded, graded the model
    against that label, and returned a confident FAILED (misplaced). Measured
    afterwards, the benchmarks in that window had not slowed down at all --
    there was nothing inside it to find, so "the model looked elsewhere" was a
    statement about the experiment and not about the model.

    A model that flags nothing would have graded BLIND under the old grader.
    Here the same model must grade UNGRADED, because a blind verdict against
    an absent stimulus is a fabricated finding.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    stimulus = landed_stimulus(positions, 41, 60, window=(1.00, 1.05))
    result = gp.grade(StubModel([]), {"trace": "ignored"}, positions,
                      41, 60, stimulus)
    check("an unloaded window is UNGRADED, not BLIND",
          result["verdict"], gp.GRADE_UNGRADED)
    check_true("and the reason names the missing stimulus",
               "never reached the window" in result["reason"])
    check("the window's measured strength is reported",
          result["stimulus"]["landed"], False)


def test_grader_refuses_a_run_whose_load_escaped_the_window():
    """A slowed tail makes the clean region uncleanable, so nothing is gradable.

    This is the other half of the first run's failure, and the dangerous half:
    the load had escaped into the tail, so the model's flags out there were
    *correct* -- and the grader scored every one of them as a false positive
    and convicted the model on the total. A grader that can convict a model
    for being right is worse than no grader.

    The check is therefore not merely that the verdict is UNGRADED, but that
    it is UNGRADED *instead of* the MISPLACED this same input would otherwise
    produce.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    model = StubModel(list(range(41, 61)) + list(range(70, 86)))
    escaped = landed_stimulus(positions, 41, 60, post=(1.40, 2.30))
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60, escaped)
    check("a load that escaped its window is UNGRADED",
          result["verdict"], gp.GRADE_UNGRADED)
    check_true("and the reason says the clean region was not clean",
               "escaped the window" in result["reason"])
    check("the escape is recorded", result["stimulus"]["escaped"], True)

    # The identical model and window, with a tail that really was clean.
    contained = landed_stimulus(positions, 41, 60)
    result = gp.grade(model, {"trace": "ignored"}, positions, 41, 60, contained)
    check("...where a genuinely clean tail would have convicted it",
          result["verdict"], gp.GRADE_MISPLACED)


def test_a_confessed_broken_window_outranks_every_other_verdict():
    """P22's second run: the trigger fired, the release name never existed.

    The `--until` name was taken from the end-of-run scorecard, and 27 of the
    86 scorecard names never appear as live result lines -- which are the only
    lines the controller can match. So the load switched on at the labelled
    place and then ran to the end of the boot. The label said "positions
    61-73"; the applied window was "61 to the end".

    That is not a fact about the model, and it must not be reported as one --
    including when the benchmarks happen to look good. A run whose window is
    not the window on the label is worth exactly nothing either way, so the
    confession has to outrank SUPPORTED just as firmly as it outranks BLIND.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    stimulus = landed_stimulus(positions, 41, 60)

    # The model that would otherwise score a clean SUPPORTED.
    good = StubModel(list(range(41, 61)))
    check("without a confession this run is SUPPORTED",
          gp.grade(good, {"trace": "x"}, positions, 41, 60,
                   stimulus)["verdict"], gp.GRADE_SUPPORTED)
    result = gp.grade(good, {"trace": "x"}, positions, 41, 60, stimulus,
                      record_problem="until-never-matched")
    check("a confessed broken window is UNGRADED even when the model looks "
          "perfect", result["verdict"], gp.GRADE_UNGRADED)
    check_true("and the reason names the missing right-hand edge",
               "no right-hand edge" in result["reason"])

    # ...and it outranks the measured-stimulus complaint, which would
    # otherwise send the reader hunting for the wrong fault.
    unloaded = landed_stimulus(positions, 41, 60, window=(1.00, 1.05))
    result = gp.grade(StubModel([]), {"trace": "x"}, positions, 41, 60,
                      unloaded, record_problem="at-never-matched")
    check("a confession outranks the measured-stimulus complaint",
          result["verdict"], gp.GRADE_UNGRADED)
    check_true("and the reason blames the trigger, not the placement",
               "never matched a live benchmark line" in result["reason"])
    check_true("...and specifically not the placement",
               "never reached the window" not in result["reason"])


def test_only_a_recognised_confession_is_admitted():
    """Absence of a confession is never read as evidence of innocence.

    The controller is the claim under test, so `load_record_problem` may only
    ever *add* a reason to distrust a run. A missing file, an unreadable one,
    or one reporting a problem this grader does not understand all return
    None -- leaving the measured stimulus gate to do its job -- rather than
    excusing or condemning the run on the record's say-so.
    """
    gp = load_module(SCRIPT, "grade_positional")
    check("no path is not a confession", gp.load_record_problem(None), None)
    check("a missing file is not a confession",
          gp.load_record_problem(os.path.join(tempfile.gettempdir(),
                                              "p22-no-such-record.json")), None)
    for label, body in (("unparseable", "{not json"),
                        ("problem-free", '{"fired": true, "released": true}'),
                        ("unknown problem", '{"problem": "gremlins"}')):
        handle, path = tempfile.mkstemp(suffix=".json")
        try:
            with os.fdopen(handle, "w") as out:
                out.write(body)
            check(f"a {label} record is not a confession",
                  gp.load_record_problem(path), None)
        finally:
            os.unlink(path)
    handle, path = tempfile.mkstemp(suffix=".json")
    try:
        with os.fdopen(handle, "w") as out:
            json.dump({"problem": "until-never-matched"}, out)
        check("a recognised problem is admitted",
              gp.load_record_problem(path), "until-never-matched")
    finally:
        os.unlink(path)


def test_a_confession_is_derived_when_the_record_does_not_state_it():
    """The fault is detected from the record's facts, not its self-summary.

    P22's second run was recorded by a controller that had no `problem` field
    yet -- the field was added *because* of that run. Its record nonetheless
    states plainly that a `--until` was asked for and never released, and
    that is the whole of the fault. Reading it out of the booleans means the
    grader keeps working on old records, and cannot be talked out of a
    verdict by a controller that simply declines to accuse itself.
    """
    gp = load_module(SCRIPT, "grade_positional")
    derive = gp.record_problem
    check("a --until that never released is derived",
          derive({"at": "b40", "fired": True,
                  "until": "b60", "released": False}), "until-never-matched")
    check("a --at that never fired is derived",
          derive({"at": "b40", "fired": False}), "at-never-matched")
    check("...and outranks the release, which could not fire either",
          derive({"at": "b40", "fired": False,
                  "until": "b60", "released": False}), "at-never-matched")
    check("a complete run is not a confession",
          derive({"at": "b40", "fired": True,
                  "until": "b60", "released": True}), None)
    check("an open-ended window is not a confession",
          derive({"at": "b40", "fired": True, "released": False}), None)
    check("a stated problem still wins over derivation",
          derive({"problem": "at-never-matched", "at": "b40", "fired": True,
                  "until": "b60", "released": False}), "at-never-matched")

    # The real thing: the preserved record from P22's second attempt, which
    # this grader must be able to convict on its own reading.
    real = os.path.join(REPO_ROOT, "build", "canary-load-record.json")
    if os.path.exists(real):
        check("P22 run 2's own record is read as a broken window",
              gp.load_record_problem(real), "until-never-matched")


def test_end_to_end_a_confessed_broken_window_is_ungraded_and_exhibited():
    """The verdict cites the record, so the record must be shown with it."""
    gp = load_module(SCRIPT, "grade_positional")
    record = {"fired": True, "released": False, "outcome": "ran-to-end",
              "problem": "until-never-matched",
              "load_seconds": 0.21, "suite_seconds_seen": 6.3,
              "completions_during": 3, "completions_before_on": 41,
              "host_probe": {"inflation": 3.11, "samples": 120}}
    handle, record_path = tempfile.mkstemp(suffix=".json")
    try:
        with os.fdopen(handle, "w") as out:
            json.dump(record, out)
        with synthetic_run() as args:
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                rc = gp.main(args + ["--load-at", "b40", "--load-until", "b60",
                                     "--load-record", record_path])
            text = buf.getvalue()
            check("a confessed broken window does not exit 0", rc, 1)
            check_true("the verdict is UNGRADED", gp.GRADE_UNGRADED in text)
            check_true("and it is not blamed on the model",
                       gp.GRADE_BLIND not in text
                       and gp.GRADE_MISPLACED not in text)
            check_true("the record is exhibited alongside the accusation",
                       "PROBLEM" in text)
            check_true("the host canary reading is shown", "x3.11" in text)
    finally:
        os.unlink(record_path)


def test_a_region_is_disturbed_only_when_both_statistics_agree():
    """Either statistic alone moves this far between two undisturbed runs.

    The measured null (see `STIMULUS_DISTURBED_MIN`) has the mean ratio's p95
    at 1.68 and the best-case ratio's max at 1.61 over clean region-pairs, so a
    single-statistic gate at these thresholds fires on clean runs several
    percent of the time. Requiring both is what buys the margin, and a future
    edit that relaxes the conjunction to an `any()` would look like a
    sensitivity improvement while quietly restoring the rubber stamp.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    for label, window in (("best-case alone", (1.40, 1.10)),
                          ("mean alone", (1.05, 2.00))):
        stimulus = landed_stimulus(positions, 41, 60, window=window)
        result = gp.grade(StubModel(range(41, 61)), {"trace": "ignored"},
                          positions, 41, 60, stimulus)
        check(f"{label} does not count as a disturbance",
              result["verdict"], gp.GRADE_UNGRADED)
    # Both together, at the same thresholds, do.
    stimulus = landed_stimulus(positions, 41, 60, window=(1.15, 1.30))
    result = gp.grade(StubModel(range(41, 61)), {"trace": "ignored"},
                      positions, 41, 60, stimulus)
    check("both at exactly the threshold is a disturbance",
          result["verdict"], gp.GRADE_SUPPORTED)


def test_each_region_too_thin_to_measure_is_named_separately():
    """Four ways to have no measurable stimulus, four different remedies.

    "The history has no baseline for this suite" needs a baseline run; "the
    window sits at the end of the suite" needs the window moved. Collapsing
    them into one 'ungradeable' message would leave the operator re-running an
    experiment that cannot succeed.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    model = StubModel(range(41, 61))

    cases = {
        # No benchmark anywhere has a baseline in the recent clean history.
        "no-baseline": (41, 60, {"min": {}, "mean": {}}),
        # The window starts at position 2, leaving two clean benchmarks in
        # front of it -- too few to establish what this run's own speed was.
        "no-pre": (2, 10, landed_stimulus(positions, 2, 10)),
        # Baselines exist, but not for anything inside the window.
        "no-window": (41, 60, landed_stimulus(
            positions, 41, 60, omit=lambda p: 41 <= p <= 60)),
        # The window ends so late that nothing runs beyond its reach, so a
        # load that never stopped is indistinguishable from a contained one.
        "no-post": (41, 80, landed_stimulus(positions, 41, 80)),
    }
    for expected, (lo, hi, stimulus) in cases.items():
        result = gp.grade(model, {"trace": "ignored"}, positions, lo, hi,
                          stimulus)
        check(f"{expected} is UNGRADED", result["verdict"], gp.GRADE_UNGRADED)
        check(f"{expected} is diagnosed as itself",
              result["stimulus"]["problem"], expected)
        check_true(f"{expected} produces its own message",
                   result["reason"] == gp.STIMULUS_PROBLEMS[expected](
                       result["stimulus"]))


def test_an_unmeasured_region_is_not_printed_as_a_clean_one():
    """"not disturbed" must not be printed next to numbers that say otherwise.

    A window placed at the end of the suite grades UNGRADED for want of a tail
    to check the load escaped into -- and `landed` is then `None`, never having
    been asked. Rendering `None` as "not disturbed" produced a printout reading
    `x1.43 best-case / x2.28 mean -- not disturbed`, which is not a summary of
    the line above it but a contradiction of it, and the reader who believes
    the word over the numbers reaches the wrong conclusion about the run.
    """
    gp = load_module(SCRIPT, "grade_positional")
    positions = synthetic_positions()
    # Window at the very end: nothing runs beyond its reach, so neither
    # question gets asked, but the window itself is plainly slower.
    stimulus = landed_stimulus(positions, 41, 85, window=(1.43, 2.28))
    result = gp.grade(StubModel(range(41, 86)), {"trace": "ignored"},
                      positions, 41, 85, stimulus)
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        gp.print_grade(result, positions, "b40", "b85")
    text = buf.getvalue()
    check("the run is UNGRADED for want of a tail",
          result["stimulus"]["problem"], "no-post")
    check_true("the measured inflation is still shown", "x1.43" in text)
    check_true("but is not labelled 'not disturbed'",
               "not disturbed" not in text)
    check_true("it is labelled as never having been asked",
               "not measured" in text)


# --- the gate's thresholds, re-derived from the real history --------------

def _clean_pairs(bh, records):
    """Consecutive pairs of clean release runs, from the best-attested host.

    The host with the most records rather than `platform.node()`, deliberately:
    the null distribution is a property of the recorded history, not of
    whichever machine happens to be running the tests, and a test that
    silently skips itself off the benchmark machine is a test that stops
    guarding the threshold exactly where the threshold is used.
    """
    hosts = {}
    for record in records:
        hosts.setdefault(record.get("host"), 0)
        hosts[record.get("host")] += 1
    if not hosts:
        return "", []
    host = max(hosts, key=lambda h: hosts[h])
    clean = [record for record in bh.comparable_records(records, host,
                                                        "release")
             if record.get("entries") and record.get("mean_ns")]
    return host, list(zip(clean, clean[1:]))


def null_region_rate(gp, bh, records):
    """How often the two-metric gate fires on runs with no stimulus at all.

    Mirrors `stimulus_strength` exactly -- region medians of per-benchmark
    inflation, each divided by a reference region's median, both statistics
    required -- and applies it where the answer is known in advance to be "not
    disturbed": between two consecutive *clean* runs on the same host.

    Each pair's suite is cut into `STIMULUS_NULL_BLOCKS` contiguous blocks by
    name order. Name order is arbitrary, which is the point: the null is about
    how much two groups of benchmarks drift apart between runs, and any
    partition into disjoint groups measures that. The first block plays the
    grader's clean prefix and the rest are tested against it, giving four
    region-pairs per run-pair.

    Returns `(fired, tested, {metric: sorted ratios})`.
    """
    fired = tested = 0
    ratios = {metric: [] for metric in gp.METRICS}
    for previous, current in _clean_pairs(bh, records)[1]:
        names = sorted(set(current["entries"]) & set(previous["entries"]))
        inflation = {
            metric: gp.measure_inflation(current[gp.METRICS[metric][1]],
                                         previous[gp.METRICS[metric][1]])
            for metric in gp.METRICS
        }
        count = len(names)
        edges = [round(count * i / gp.STIMULUS_NULL_BLOCKS)
                 for i in range(gp.STIMULUS_NULL_BLOCKS + 1)]

        def median_of(metric, lo, hi, _names=names, _inf=inflation):
            values = [_inf[metric][n] for n in _names[lo:hi] if n in _inf[metric]]
            if len(values) < gp.STIMULUS_MIN_SAMPLES:
                return None
            return statistics.median(values)

        reference = {metric: median_of(metric, edges[0], edges[1])
                     for metric in gp.METRICS}
        if any(value is None for value in reference.values()):
            continue
        for block in range(1, gp.STIMULUS_NULL_BLOCKS):
            metrics = {}
            for metric in gp.METRICS:
                value = median_of(metric, edges[block], edges[block + 1])
                metrics[metric] = {
                    "ratio": None if value is None else value / reference[metric]
                }
            if any(m["ratio"] is None for m in metrics.values()):
                continue
            tested += 1
            for metric in gp.METRICS:
                ratios[metric].append(metrics[metric]["ratio"])
            if gp.region_disturbed(metrics, "ratio"):
                fired += 1
    return fired, tested, {m: sorted(v) for m, v in ratios.items()}


def test_the_stimulus_gate_is_still_above_the_noise_floor():
    """Re-derive the gate's false-positive rate from the real history.

    `STIMULUS_DISTURBED_MIN` and `STIMULUS_DISTURBED_MEAN` are justified by a
    measurement, and a measurement nobody repeats is a justification that
    expires. If the suite gets noisier -- more benchmarks, a flakier one added,
    a host that starts thermal-throttling -- the same thresholds quietly become
    a rubber stamp, and every subsequent P22 run reports a stimulus that was
    never applied. That is the precise failure this whole gate was built to
    prevent, so it must not be allowed to return through the back door.
    """
    gp = load_module(SCRIPT, "grade_positional")
    bh = load_module(BENCH_HISTORY, "bench_history_for_null")
    records = bh.load_history(REAL_HISTORY)
    host, pairs = _clean_pairs(bh, records)
    if not check_true(f"the history has clean release pairs to measure "
                      f"(host {host!r}, {len(pairs)} pair(s))",
                      len(pairs) >= 10):
        return
    fired, tested, ratios = null_region_rate(gp, bh, records)
    rate = fired / tested if tested else 1.0
    for metric, values in ratios.items():
        p95 = values[min(len(values) - 1, int(0.95 * len(values)))]
        print(f"      null {metric:>4}: median "
              f"{statistics.median(values):.3f}  p95 {p95:.3f}  "
              f"max {max(values):.3f}")
    print(f"      gate fired on {fired} of {tested} clean region-pairs "
          f"({100 * rate:.1f}%; documented {100 * gp.STIMULUS_NULL_RATE:.1f}%)")
    check_true(f"enough clean region-pairs to measure a rate ({tested})",
               tested >= 100)
    check_true(f"the gate's false-positive rate {100 * rate:.1f}% is still "
               f"under the {100 * gp.STIMULUS_NULL_RATE_CEILING:.0f}% ceiling",
               rate <= gp.STIMULUS_NULL_RATE_CEILING)
    # The mirror-image failure: a gate nothing can trip is not conservative,
    # it is broken. A real disturbance measured x1.40 / x2.30, so the same
    # arithmetic applied to that must fire.
    check_true("and the gate still fires on a real disturbance's numbers",
               gp.region_disturbed({"min": {"r": 1.40}, "mean": {"r": 2.30}},
                                   "r"))


# --- end to end, on the real model, over a synthetic log ------------------

#: The disturbance the synthetic run carries, as a factor on each statistic.
#: Comfortably clear of `STIMULUS_DISTURBED_MIN`/`_MEAN` and of the measured
#: null: these tests are about the wiring, so the stimulus must never be the
#: marginal thing that decides them.
SYNTH_SLOWDOWN = {"min": 1.5, "mean": 2.0}
SYNTH_HOST = "synthetic-host"


def _synthetic_baseline(count=86):
    """What the previous, clean run measured: the undisturbed timings.

    These are the same numbers `_synthetic_serial` starts from before it
    applies `SYNTH_SLOWDOWN`, so the inflation the grader measures is exactly
    the slowdown inside the window and exactly 1.0 outside it -- there is no
    baseline noise here to reason about, deliberately, because these tests are
    about the wiring and not about the thresholds.
    """
    entries, means = {}, {}
    for i in range(count):
        entries[f"b{i:02d}"] = 100 + i
        means[f"b{i:02d}"] = 200 + i
    return {
        "host": SYNTH_HOST,
        "profile": "release",
        "commit": "0000000",
        "host_load": "idle",
        "entries": entries,
        "mean_ns": means,
    }


def _synthetic_serial(count=86, elevated=(48, 56), base="5.00", high="15.00",
                      slow=range(41, 61)):
    """A serial log shaped like a run disturbed only across one window.

    Two independent things are made to agree here, and the grader checks each
    against the other:

      * `slow` raises the *benchmark timings*, which is the stimulus -- the
        ground truth of whether the window really was loaded;
      * `elevated` raises the *canary trace*, which is what the model reads.

    They are separate arguments on purpose. Passing `elevated=()` while
    leaving `slow` alone describes a run where the load landed and the model
    missed it (a real BLIND), and only by keeping them separable can the
    negative control below be a control rather than a tautology.
    """
    lines = []
    for i in range(count):
        factor = SYNTH_SLOWDOWN if i in slow else {"min": 1.0, "mean": 1.0}
        measured = round((100 + i) * factor["min"])
        mean = round((200 + i) * factor["mean"])
        lines.append(f"[bench] SCORE b{i:02d} {measured} - TRACK {mean} 500 0")
    samples = []
    for pos in range(0, count, 8):
        samples.append(f"{pos}:{high if pos in elevated else base}")
    samples.append(f"start:{base}")
    samples.append(f"end:{base}")
    # start end pct min max spread samples invalid min_centi max_centi
    canary = "[bench] CANARY 5 5 100 5 15 200 13 0 500 1500"
    trace = "[bench] CANARY-TRACE " + " ".join(samples)
    return "\n".join(lines + [canary, trace]) + "\n"


@contextlib.contextmanager
def synthetic_run(**kwargs):
    """A serial log and the one-record history that gives it a baseline.

    Yields the `gp.main` arguments that point at both. The history is
    synthetic rather than the real `bench/history.jsonl` because the grader
    now measures the stimulus against recent clean runs *of the same
    benchmarks*: fed the real history, `b00..b85` would have no baseline at
    all and every end-to-end test would grade UNGRADED for the wrong reason.
    """
    serial_fd, serial = tempfile.mkstemp(suffix=".txt")
    history_fd, history = tempfile.mkstemp(suffix=".jsonl")
    try:
        with os.fdopen(serial_fd, "w") as out:
            out.write(_synthetic_serial(**kwargs))
        with os.fdopen(history_fd, "w") as out:
            out.write(json.dumps(_synthetic_baseline()) + "\n")
        yield ["--serial", serial, "--history", history, "--host", SYNTH_HOST,
               "--profile", "release"]
    finally:
        os.unlink(serial)
        os.unlink(history)


def test_end_to_end_on_a_synthetic_localised_disturbance():
    """Smoke test of the whole chain: log -> positions -> model -> verdict.

    Synthetic, and therefore NOT evidence for P22 -- the trace here was
    written to be the right answer. What it does establish is that
    `parse_serial`'s positions, `parse_canary`'s trace, the baseline lookup,
    the stimulus measurement and the interpolation actually join up, which is
    the part most likely to break silently when any of them changes.
    """
    gp = load_module(SCRIPT, "grade_positional")
    with synthetic_run() as args:
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = gp.main(args + ["--load-at", "b40", "--load-until", "b60"])
        text = buf.getvalue()
        check("localised synthetic disturbance grades SUPPORTED and exits 0",
              rc, 0)
        check_true("the verdict is printed", gp.GRADE_SUPPORTED in text)
        check_true("the window is stated", "positions 41-60" in text)
        check_true("the resolution limit is stated", "widened" in text)
        check_true("the measured stimulus is reported before the score",
                   text.index("loaded window") < text.index("Model flagged"))
        check_true("and the window is reported as disturbed",
                   "DISTURBED" in text)


def test_end_to_end_flat_trace_is_not_reported_as_a_hit():
    """An undisturbed run must not grade as SUPPORTED just because load ran.

    The negative control for the test above. If a flat trace produced a
    SUPPORTED verdict, every run would confirm P22 regardless of what
    happened, which is the failure the whole file is written against.

    Note that the *timings* are still raised: this is a run where the load
    demonstrably landed and the model saw nothing, which is the only
    circumstance under which BLIND is a statement about the model.
    """
    gp = load_module(SCRIPT, "grade_positional")
    with synthetic_run(elevated=()) as args:
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = gp.main(args + ["--load-at", "b40", "--load-until", "b60"])
        check("a flat trace does not grade SUPPORTED", rc, 1)
        check_true("and says the model saw nothing",
                   gp.GRADE_BLIND in buf.getvalue())


def test_end_to_end_an_unloaded_window_is_ungraded_not_blind():
    """The whole-chain version of the run that invalidated P22's first attempt.

    Same flat trace as the control above, but the benchmarks in the labelled
    window did not slow down either. The model's silence is then unremarkable
    -- there was nothing to notice -- and reporting it as FAILED (blind) would
    be manufacturing a refutation out of a broken stimulus, which is precisely
    what happened the first time this experiment was run for real.
    """
    gp = load_module(SCRIPT, "grade_positional")
    with synthetic_run(elevated=(), slow=()) as args:
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = gp.main(args + ["--load-at", "b40", "--load-until", "b60"])
        text = buf.getvalue()
        check("an unloaded window does not grade at all", rc, 1)
        check_true("it is UNGRADED", gp.GRADE_UNGRADED in text)
        check_true("and not blamed on the model", gp.GRADE_BLIND not in text)
        check_true("the reason names the stimulus",
                   "never reached the window" in text)


def test_end_to_end_reports_the_load_records_own_account_separately():
    """The load script's claim is printed, and printed as a diagnostic.

    It is the thing under suspicion -- the first run's record asserted a
    correctly-placed load that the timings say never arrived -- so it must
    appear after the measured grade and be labelled as not evidence.
    """
    gp = load_module(SCRIPT, "grade_positional")
    record = {"fired": True, "released": True, "outcome": "released",
              "load_seconds": 0.21, "suite_seconds_seen": 6.3,
              "completions_during": 3, "completions_before_on": 41}
    handle, record_path = tempfile.mkstemp(suffix=".json")
    try:
        with os.fdopen(handle, "w") as out:
            json.dump(record, out)
        with synthetic_run() as args:
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                gp.main(args + ["--load-at", "b40", "--load-until", "b60",
                                "--load-record", record_path])
            text = buf.getvalue()
            check_true("the record is labelled as not evidence",
                       "diagnostic, not evidence" in text)
            check_true("the window's share of the suite is worked out",
                       "3.3%" in text)
            check_true("and it comes after the verdict",
                       text.index("VERDICT") < text.index("Load controller"))
    finally:
        os.unlink(record_path)


def test_positions_only_mode_reports_the_window_and_nothing_else():
    gp = load_module(SCRIPT, "grade_positional")
    with synthetic_run() as args:
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = gp.main(args + ["--load-at", "b40", "--load-until", "b60",
                                 "--positions-only", "--label", "predicted"])
        text = buf.getvalue()
        check("--positions-only exits 0", rc, 0)
        check_true("reports the window", "positions 41-60" in text)
        check_true("carries the label", "predicted" in text)
        check_true("does not grade", gp.GRADE_SUPPORTED not in text)


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
