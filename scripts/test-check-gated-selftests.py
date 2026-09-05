#!/usr/bin/env python3
"""Regression tests for the never-ran-gated-self-test gate.

Run: `python scripts/test-check-gated-selftests.py` (0 = pass, 1 = fail). No
pytest dependency, matching the other suites in this directory, so it runs from
a bare checkout and from `scripts/boot-test.sh`.

What this gate is for, and what makes it dangerous
--------------------------------------------------

`check-gated-selftests.py` reads the `gated_ran` field that `boot-history.py`
records per boot -- for each `RAN-IF` marker declared in `kernel/src/main.rs`,
whether that serial line appeared -- and fails a marker that has never once
been seen. The bug class it catches is a self-test that is correctly wired,
correctly summarised as PASSED, and never executed, because the `if` around its
call site has never been true.

The gate's own dangerous direction is the opposite one: a **false accusation**.
Reporting a working suite as never-run sends someone to debug code that is
fine, and unlike a missed finding it is loud, so it gets acted on. Almost every
test below pins one specific way the arithmetic could produce that:

  * counting a boot that predates the field, or died early, or was a probe, as
    a boot on which the suite did not run;
  * counting the *window* as the denominator, so a marker declared yesterday
    reads as never-run out of twenty-five;
  * failing on behalf of a marker whose call site has since been deleted;
  * failing a fresh allowlist entry for being fresh.

The remaining tests pin the two ways the gate could go quietly blind: a marker
set that empties out, and an allowlist entry that outlives its truth.
"""

from __future__ import annotations

import inspect
import json
import os
import sys
import tempfile

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SCRIPT_DIR)

import srcload  # noqa: E402

gate = srcload.load(os.path.join(SCRIPT_DIR, "check-gated-selftests.py"),
                    "check_gated_selftests_under_test")

_FAILURES: list[str] = []

M1 = "[fat] Running mkfs/format self-test..."
M2 = "[acpi] Running self-test..."


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def _rows(n: int, ran: dict, **over) -> list[dict]:
    """`n` identical qualifying rows whose `gated_ran` is `ran`."""
    rec = {"boot_ok": True, "verdict": "PASS"}
    rec.update(over)
    return [dict(rec, gated_ran=dict(ran)) for _ in range(n)]


# --------------------------------------------------------------------------
# The core question
# --------------------------------------------------------------------------

def test_below_the_floor_no_verdict():
    res = gate.analyse(_rows(9, {M1: False}), 25, 10)
    check("nine boots without the line is not yet an accusation",
          (res["never"], res["undecided"]), ([], [(M1, 9)]))


def test_at_the_floor_it_fires():
    res = gate.analyse(_rows(10, {M1: False}), 25, 10)
    check("ten boots, never seen, is the finding",
          (res["never"], res["undecided"]), ([M1], []))


def test_one_sighting_is_a_complete_defence():
    # The claim is "never once observed". A single True refutes it outright,
    # and no amount of later absence rebuilds it -- a conditional suite is
    # *allowed* to be conditional; it is only silence forever that is the bug.
    rows = _rows(1, {M1: True}) + _rows(20, {M1: False})
    res = gate.analyse(rows, 25, 10)
    check("one boot on which the line appeared clears the charge",
          (res["never"], res["counts"][M1]), ([], (1, 21)))


def test_markers_are_judged_independently():
    rows = _rows(12, {M1: False, M2: True})
    res = gate.analyse(rows, 25, 10)
    check("a marker that always appears does not vouch for one that never does",
          (res["never"], sorted(res["live"])), ([M1], sorted([M1, M2])))


def test_window_forgets_old_boots():
    # The precondition got fixed. Once the old rows fall out of the window the
    # gate must stop failing, or it teaches its reader to pass --no-verify.
    rows = _rows(30, {M1: False}) + _rows(25, {M1: True})
    res = gate.analyse(rows, 25, 10)
    check("a marker that started appearing leaves the window",
          res["never"], [])


# --------------------------------------------------------------------------
# The denominator, which is per marker and has to be
# --------------------------------------------------------------------------

def test_a_new_marker_is_not_accused_of_its_own_novelty():
    # Twenty-five boots in the window, three of which knew about M2. Using the
    # window as the denominator makes a marker introduced today read as
    # never-run on twenty-five boots -- the gate failing loudest at the exact
    # moment someone declares a marker correctly.
    rows = _rows(22, {M1: True}) + _rows(3, {M1: True, M2: False})
    res = gate.analyse(rows, 25, 10)
    check("a marker is measured only against the boots that recorded it",
          (res["never"], res["undecided"], res["counts"][M2]),
          ([], [(M2, 3)], (0, 3)))


def test_the_floor_applies_to_the_marker_not_the_window():
    # Same shape as above but with ten qualifying boots for M2, which is the
    # floor: now it is a finding, even though most of the window never heard
    # of it.
    rows = _rows(15, {M1: True}) + _rows(10, {M1: True, M2: False})
    res = gate.analyse(rows, 25, 10)
    check("ten boots carrying the marker is enough, whatever the window size",
          (res["never"], res["n"]), ([M2], 25))


# --------------------------------------------------------------------------
# Rows that are not evidence
# --------------------------------------------------------------------------

def test_rows_without_the_field_are_not_evidence():
    # A pre-field row has no opinion about anything. Reading its absence as
    # "the suite did not run" would manufacture a finding out of history.
    old = [{"boot_ok": True, "verdict": "PASS"} for _ in range(50)]
    res = gate.analyse(old + _rows(10, {M1: True}), 25, 10)
    check("rows predating the field neither count nor dilute",
          (res["n"], res["never"]), (10, []))


def test_an_empty_gated_ran_is_a_row_not_a_gap():
    # `boot-history.py` omits the field when it cannot read the markers file,
    # and writes `{}` only when there genuinely are no markers. The two must
    # not collapse: `{}` is a real observation of a tree with nothing to check.
    res = gate.analyse(_rows(10, {}), 25, 10)
    check("an empty dict qualifies as a boot and finds nothing",
          (res["n"], res["live"], res["never"]), (10, [], []))


def test_failed_boots_are_not_evidence():
    dead = _rows(50, {M1: False}, boot_ok=False, verdict="TIMEOUT")
    res = gate.analyse(dead + _rows(10, {M1: True}), 25, 10)
    check("a boot that died early never reached the call site",
          (res["n"], res["never"]), (10, []))


def test_experiments_are_not_evidence():
    probe = _rows(50, {M1: False}, experiment="-cpu host")
    res = gate.analyse(probe + _rows(10, {M1: True}), 25, 10)
    check("a probe is evidence about the probe",
          (res["n"], res["never"]), (10, []))


def test_a_non_dict_gated_ran_is_ignored():
    # Defensive: the field is merged across three lanes and hand-edited rows
    # have appeared before. A list where a dict belongs must not crash the gate
    # for the several hundred good rows around it.
    junk = [dict(boot_ok=True, gated_ran=["[x]"]) for _ in range(5)]
    res = gate.analyse(junk + _rows(10, {M1: False}), 25, 10)
    check("a malformed field is discarded, not parsed",
          (res["n"], res["never"]), (10, [M1]))


# --------------------------------------------------------------------------
# Markers that no longer exist
# --------------------------------------------------------------------------

def test_a_retired_marker_is_not_failed():
    # M2's call site was deleted. Ten boots recorded it as never-seen, and it
    # would fail forever -- an accusation with no address, whose only remedy is
    # to allowlist a line that is not in the tree.
    rows = _rows(10, {M1: True, M2: False}) + _rows(1, {M1: True})
    res = gate.analyse(rows, 25, 10)
    check("a marker the current tree no longer declares is retired, not failed",
          (res["never"], res["retired"], res["live"]), ([], [M2], [M1]))


def test_retirement_is_read_from_the_newest_boot():
    # The live set is the newest row's keys, so a marker *added* in the newest
    # boot is live from the first row that carries it.
    rows = _rows(10, {M1: True}) + _rows(1, {M1: True, M2: False})
    res = gate.analyse(rows, 25, 10)
    check("a marker declared only by the newest boot is live",
          (sorted(res["live"]), res["retired"], res["undecided"]),
          (sorted([M1, M2]), [], [(M2, 1)]))


# --------------------------------------------------------------------------
# The allowlist, in both directions
# --------------------------------------------------------------------------

def _allowed(monkey: dict):
    """Swap ALLOWED for the duration of one test."""
    prev = gate.ALLOWED
    gate.ALLOWED = monkey
    return prev


def test_allowlisted_marker_is_reported_not_failed():
    prev = _allowed({M1: "needs an optical drive attached"})
    try:
        res = gate.analyse(_rows(10, {M1: False}), 25, 10)
        check("an allowlisted never-run suite is a note, not a failure",
              (res["never"], res["allowed_never"], res["stale_allowlist"]),
              ([], [M1], []))
    finally:
        gate.ALLOWED = prev


def test_a_fresh_allowlist_entry_is_not_stale():
    # Three boots is below the floor, so without care M1 lands in `undecided`,
    # is therefore absent from the confirmed set, and gets failed as stale --
    # a new entry rejected for being new, with no way to add one correctly.
    prev = _allowed({M1: "needs an optical drive attached"})
    try:
        res = gate.analyse(_rows(3, {M1: False}), 25, 10)
        check("an allowlist entry added before ten boots is not stale",
              (res["stale_allowlist"], res["allowed_never"]), ([], [M1]))
    finally:
        gate.ALLOWED = prev


def test_an_allowlist_entry_that_started_running_fails():
    prev = _allowed({M1: "needs an optical drive attached"})
    try:
        res = gate.analyse(_rows(9, {M1: False}) + _rows(1, {M1: True}), 25, 10)
        check("an entry contradicted by a sighting is a failure",
              (res["stale_allowlist"], res["never"]), ([M1], []))
    finally:
        gate.ALLOWED = prev


def test_an_allowlist_entry_naming_nothing_fails():
    prev = _allowed({"[gone] Running self-test...": "needs a second core"})
    try:
        res = gate.analyse(_rows(10, {M1: True}), 25, 10)
        check("an entry for a marker no call site declares is a failure",
              res["stale_allowlist"], ["[gone] Running self-test..."])
    finally:
        gate.ALLOWED = prev


def test_allowlist_entries_state_a_condition():
    # The docstring's rule, enforced: an entry that does not say what would
    # make the suite start running is the beginning of a dumping ground. Vacuous
    # while ALLOWED is empty, and deliberately kept so the rule is already in
    # force on the day the first entry is written.
    bad = [k for k, v in gate.ALLOWED.items() if len(v) < 60]
    check("every allowlist entry carries a real justification", bad, [])


# --------------------------------------------------------------------------
# Going blind
# --------------------------------------------------------------------------

def test_the_marker_set_emptying_out_is_visible():
    # An empty `markers` object writes an empty `gated_ran`, after which every
    # marker is retired and the gate can never accuse anyone again. It must not
    # report that as a clean OK.
    rows = _rows(10, {M1: False}) + _rows(1, {})
    res = gate.analyse(rows, 25, 10)
    check("a newest boot declaring nothing leaves the earlier markers retired",
          (res["live"], res["retired"], res["never"]), ([], [M1], []))


def test_no_history_is_no_verdict_not_a_pass(tmpdir):
    path = os.path.join(tmpdir, "empty.jsonl")
    with open(path, "w", encoding="utf-8", newline="") as handle:
        handle.write("")
    check("an empty history exits 0", gate.main(["--history", path]), 0)


# --------------------------------------------------------------------------
# main()
# --------------------------------------------------------------------------

def _write(path: str, rows: list[dict]) -> None:
    with open(path, "w", encoding="utf-8", newline="") as handle:
        for rec in rows:
            handle.write(json.dumps(rec) + "\n")


def test_main_fails_on_a_real_finding(tmpdir):
    path = os.path.join(tmpdir, "h.jsonl")
    _write(path, _rows(10, {M1: False}))
    check("a never-seen marker exits 1", gate.main(["--history", path]), 1)


def test_main_passes_on_a_healthy_history(tmpdir):
    path = os.path.join(tmpdir, "h.jsonl")
    _write(path, _rows(10, {M1: True, M2: True}))
    check("a history where every marker has been seen exits 0",
          gate.main(["--history", path]), 0)


def test_main_survives_a_malformed_line(tmpdir):
    path = os.path.join(tmpdir, "bad.jsonl")
    with open(path, "w", encoding="utf-8", newline="") as handle:
        handle.write("{not json\n")
        for rec in _rows(10, {M1: False}):
            handle.write(json.dumps(rec) + "\n")
    check("one bad line does not blind the gate to the good ones",
          gate.main(["--history", path]), 1)


def test_main_missing_history_is_exit_two(tmpdir):
    check("an unreadable history is a tool failure, not a tree failure",
          gate.main(["--history", os.path.join(tmpdir, "nope.jsonl")]), 2)


def test_list_never_fails(tmpdir):
    path = os.path.join(tmpdir, "h.jsonl")
    _write(path, _rows(10, {M1: False}) + _rows(1, {M2: False}))
    check("--list reports the same finding without failing",
          gate.main(["--history", path, "--list"]), 0)


def test_the_real_history_is_readable():
    # The gate runs against this file on every boot test; a shape it cannot
    # read is a failure of this suite, not a surprise at 3am.
    path = gate.DEFAULT_HISTORY
    if not os.path.exists(path):
        print("SKIP  the real history (no bench/boot-history.jsonl)")
        return
    res = gate.analyse(gate.load(path), 25, 10)
    check("the committed history parses and yields no false accusation",
          res["never"], [])


# --------------------------------------------------------------------------

def main():
    tests = [(n, f) for n, f in sorted(globals().items())
             if n.startswith("test_") and callable(f)]
    if len(tests) < 20:
        print(f"FATAL: test discovery found only {len(tests)} tests; the suite "
              f"has at least 20. Discovery is broken, not the code.")
        return 1
    for name, fn in tests:
        params = inspect.signature(fn).parameters
        with tempfile.TemporaryDirectory() as tmpdir:
            avail = {"tmpdir": tmpdir}
            missing = [p for p in params if p not in avail]
            if missing:
                print(f"FATAL: {name} wants {missing}, which the harness does "
                      f"not supply. Fix the harness, do not skip the test.")
                return 1
            fn(**{p: avail[p] for p in params})

    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all {len(tests)} check-gated-selftests tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
