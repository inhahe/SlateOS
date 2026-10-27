#!/usr/bin/env python3
"""Regression tests for the never-running-self-test gate.

Run: `python scripts/test-check-boot-skips.py` (0 = pass, 1 = fail). No pytest
dependency, matching the other suites in this directory, so it runs from a bare
checkout and from `scripts/boot-test.sh`.

Two halves, and the first one is where the bugs were:

  * `boot-history.py`'s **parser** -- turning a serial log into skip *names*.
    Every failure here is silent and permanent. A name that carries anything
    run-specific is never equal to itself on the next boot, so the
    100%-of-N test can never accumulate evidence and the gate reports "all
    clear" forever. The first draft of `_SKIP_RE` used `\\s*` between the tag
    and the word SKIP; `\\s` matches a newline, so it paired a tag on one line
    with a SKIP hundreds of lines later and produced names like `'[mm] Frame
    allocator self-test PASSED - 2 section(s) [mm] Kernel heap allocator
    initialized'`. The real log is therefore a fixture here, verbatim.

  * `check-boot-skips.py`'s **verdict** -- the window, the floor, and the
    allowlist in both directions.

The fixtures below are real lines from `build/serial-test.txt`, including the
nested-parenthesis reason and the closing-summary line that must not be
mistaken for a skip.
"""

from __future__ import annotations

import contextlib
import importlib.util
import inspect
import io
import json
import os
import sys
import tempfile

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SCRIPT_DIR)


def _load(name: str, filename: str):
    """Import a hyphenated script by path; its name is not an identifier."""
    spec = importlib.util.spec_from_file_location(
        name, os.path.join(SCRIPT_DIR, filename))
    if spec is None or spec.loader is None:
        raise ImportError(filename)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


bh = _load("boot_history_under_test", "boot-history.py")
gate = _load("check_boot_skips_under_test", "check-boot-skips.py")

_FAILURES: list[str] = []


def check(label, got, want):
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}")
    print(f"        got : {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)
    return False


def check_true(label, got):
    return check(label, bool(got), True)


# --------------------------------------------------------------------------
# The parser
# --------------------------------------------------------------------------

#: Verbatim from a green boot, with the surrounding lines that made the first
#: draft go wrong. Do not tidy this: the blank-looking gaps and the ordinary
#: log lines between skips are the fixture.
#:
#: Three of these skips are *covered* -- the section skips at one call site and
#: runs at a later one -- and the lines that prove it are here too, thousands
#: of lines apart in the real log. Deleting them would not make this fixture
#: tidier, it would delete the case that broke the first version of the gate.
REAL_LOG = """\
[mm] Running frame allocator self-test...
[mm]   SKIP: Zeroed frame allocation (HHDM is not mapped yet (running before page_table::init))
[mm]   SKIP: Zero-on-free (HHDM is not mapped yet (running before page_table::init))
[mm] Frame allocator self-test PASSED \u2014 2 section(s) SKIPPED
[mm] Kernel heap allocator initialized
[selftest]   SKIP: suffix rendering (exercising the singular path)
[io_ring]   SKIP: File handle read/write (/tmp is not mounted yet (running before filesystem init))
[io_ring]   SKIP: Positioned I/O (pread/pwrite) (/tmp is not mounted yet (running before filesystem init))
[io_ring]   File handle read/write (1 entry): OK
[io_ring]   Positioned I/O (pread/pwrite preserve the cursor): OK
[iso9660]   parent_path: ok
[iso9660]   No ISO 9660 filesystem mounted \u2014 skipping integration test.
[iso9660]   SKIP: integration test (no ISO 9660 filesystem mounted)
[iso9660] Self-test passed (6 tests) \u2014 1 section(s) SKIPPED.
[hotplug]   Stats: OK (online=1, total=1)
[hotplug]   Single-CPU: skipping offline/online cycle
[hotplug]   SKIP: offline/online cycle (single-CPU system)
[hotplug] Self-test PASSED \u2014 1 section(s) SKIPPED
[mm]   Zero-on-free: OK (counter=2, settled=false)
BOOT_OK
"""


def test_parses_exactly_the_named_skips():
    check("real log yields one name per SKIP: line and nothing else",
          list(bh.parse_skips(REAL_LOG)),
          ["[hotplug] offline/online cycle",
           "[io_ring] File handle read/write",
           "[io_ring] Positioned I/O (pread/pwrite)",
           "[iso9660] integration test",
           "[mm] Zero-on-free",
           "[mm] Zeroed frame allocation",
           "[selftest] suffix rendering"])


def test_nested_parens_in_the_reason():
    # Splitting at the *last* " (" yields "Zeroed frame allocation (HHDM is
    # not mapped yet" -- a name that looks plausible and is wrong.
    check("a reason with its own parentheses is stripped whole",
          bh.parse_skips(
              "[mm]   SKIP: Zeroed frame allocation (HHDM is not mapped yet "
              "(running before page_table::init))\n"),
          ("[mm] Zeroed frame allocation",))


def test_parens_belonging_to_the_name_survive():
    check("only the reason is stripped, not a paren inside the name",
          bh.parse_skips(
              "[io_ring]   SKIP: Positioned I/O (pread/pwrite) (/tmp is not "
              "mounted yet)\n"),
          ("[io_ring] Positioned I/O (pread/pwrite)",))


def test_tag_and_skip_must_be_on_one_line():
    # The `\s` bug, pinned. Two lines, no SKIP on the first: the answer is the
    # second line's skip alone, never a name spliced across the two.
    log = "[mm] Frame allocator self-test PASSED\n[mm] Kernel heap SKIPPED\n"
    check("a tag on one line never pairs with a SKIP on another",
          bh.parse_skips(log), ("[mm] Kernel heap",))


def test_closing_summary_is_not_a_skip():
    check("`N section(s) SKIPPED` is a count, not a section",
          bh.parse_skips("[mm] Frame allocator self-test PASSED \u2014 "
                         "2 section(s) SKIPPED\n"),
          ())


def test_overflow_line_is_not_a_skip():
    check("the ledger-overflow line names no section and is not counted",
          bh.parse_skips("[mm]   SKIP: 3 further section(s) "
                         "(ledger holds 8)\n"),
          ())


def test_skipped_spelling_is_recognised():
    check("`Self-test SKIPPED (why)` is the same event as `SKIP: name (why)`",
          bh.parse_skips("[backtrace] Self-test SKIPPED (no frame pointers)\n"),
          ("[backtrace] Self-test",))


def test_bench_shape_with_varying_reason():
    # `[bench] {}: SKIPPED ({:?})` puts a Debug-formatted error in the reason,
    # so the reason differs between runs and the name must not contain it.
    a = bh.parse_skips("[bench]   page_alloc_free: SKIPPED (NoMemory)\n")
    b = bh.parse_skips("[bench]   page_alloc_free: SKIPPED (OutOfRange)\n")
    check("two runs with different reasons produce one name", (a, a == b),
          (("[bench] page_alloc_free",), True))


def test_duplicate_names_collapse():
    check("a suite that runs twice contributes one name",
          bh.parse_skips("[acpi]   SKIP: table walk (no RSDP)\n"
                         "[acpi]   SKIP: table walk (no RSDP)\n"),
          ("[acpi] table walk",))


def test_a_log_with_no_skips():
    check("a boot that skipped nothing yields an empty tuple",
          bh.parse_skips("[mm] Frame allocator self-test PASSED\nBOOT_OK\n"),
          ())


def test_unbalanced_paren_is_left_alone():
    check("an unbalanced reason is not half-trimmed into a wrong name",
          bh._strip_trailing_paren("name (why"), "name (why")


def test_record_carries_skips_as_a_list(tmpdir):
    path = os.path.join(tmpdir, "serial.txt")
    with open(path, "w", encoding="utf-8", newline="") as handle:
        handle.write(REAL_LOG)
    serial = bh.read_serial(path)
    check_true("read_serial populates skips", bool(serial and serial.skips))
    check("skips reach the record as a JSON list",
          json.loads(json.dumps(list(serial.skips)))[:1],
          ["[hotplug] offline/online cycle"])


def test_record_carries_the_covered_half_too(tmpdir):
    # Both halves are recorded. `skips` alone would answer "did it skip" but
    # not "was it picked up elsewhere", and the second question is the one that
    # separates a tripwire from a defect.
    path = os.path.join(tmpdir, "serial.txt")
    with open(path, "w", encoding="utf-8", newline="") as handle:
        handle.write(REAL_LOG)
    serial = bh.read_serial(path)
    check("read_serial splits the skips in two",
          (list(serial.skips), list(serial.skips_covered)),
          (["[hotplug] offline/online cycle",
            "[iso9660] integration test",
            "[mm] Zeroed frame allocation",
            "[selftest] suffix rendering"],
           ["[io_ring] File handle read/write",
            "[io_ring] Positioned I/O (pread/pwrite)",
            "[mm] Zero-on-free"]))


# --------------------------------------------------------------------------
# The coverage split
#
# Every test here is a false positive the gate would otherwise have shipped.
# Three of the first version's four findings were wrong in exactly these ways,
# and the "fix" its message invited -- delete the pre-mount call -- would have
# destroyed a deliberate tripwire in `ipc::io_ring`.
# --------------------------------------------------------------------------

def test_a_skip_that_runs_later_is_covered():
    # The io_ring tripwire: skipped before /tmp is mounted, run after.
    unc, cov = bh.partition_skips(REAL_LOG)
    check("a section that skips at one call site and runs at another is "
          "covered, not uncovered",
          ("[io_ring] File handle read/write" in cov,
           "[io_ring] File handle read/write" in unc),
          (True, False))


def test_coverage_survives_a_differently_spelled_paren():
    # The skip says `Positioned I/O (pread/pwrite)`; the result says
    # `Positioned I/O (pread/pwrite preserve the cursor)`. Matching the whole
    # section text finds nothing and calls a section that plainly ran dead.
    log = ("[io_ring]   SKIP: Positioned I/O (pread/pwrite) (/tmp is not "
           "mounted yet)\n"
           "[io_ring]   Positioned I/O (pread/pwrite preserve the cursor): "
           "OK\n")
    check("the section is matched up to its first paren",
          bh.partition_skips(log),
          ((), ("[io_ring] Positioned I/O (pread/pwrite)",)))


def test_a_prose_skip_line_is_not_evidence_the_section_ran():
    # `[hotplug]   Single-CPU: skipping offline/online cycle` names the section
    # and carries the same tag. Read as an ordinary line it says the section
    # ran; it says the exact opposite.
    log = ("[hotplug]   Single-CPU: skipping offline/online cycle\n"
           "[hotplug]   SKIP: offline/online cycle (single-CPU system)\n")
    check("a line that narrates the skip does not excuse it",
          bh.partition_skips(log),
          (("[hotplug] offline/online cycle",), ()))


def test_a_failed_section_still_counts_as_having_run():
    # The question is whether the case executed. A failure is loud on its own
    # and reddens the boot through machinery that is not this field's.
    log = ("[fs]   SKIP: journal replay (no journal on this device)\n"
           "[fs]   journal replay: FAIL: checksum mismatch\n")
    check("FAIL is evidence of execution, not of a skip",
          bh.partition_skips(log), ((), ("[fs] journal replay",)))


def test_a_short_section_key_must_match_in_full():
    # `RX (queue 0)` truncates to the key `RX`, which occurs in half a NIC
    # driver's output. Below `_MIN_COVER_KEY` the truncation is discarded and
    # the full section text is required, so the accident cannot excuse it.
    log = ("[e1000]   SKIP: RX (queue 0) (no link)\n"
           "[e1000]   RX/TX ring sizes: OK\n")
    check("a two-character key cannot match its way to covered",
          bh.partition_skips(log), (("[e1000] RX (queue 0)",), ()))
    check_true("the floor is high enough to be doing work",
               bh._MIN_COVER_KEY >= 4)


def test_coverage_does_not_cross_tags():
    # A section name is only unique within its subsystem. `[a] self-test` must
    # not be excused by `[b] self-test: OK`.
    log = ("[a]   SKIP: ring buffer wraparound (no buffer)\n"
           "[b]   ring buffer wraparound: OK\n")
    check("another subsystem's success is not this one's coverage",
          bh.partition_skips(log),
          (("[a] ring buffer wraparound",), ()))


# --------------------------------------------------------------------------
# The verdict
# --------------------------------------------------------------------------

def _rows(n: int, skips, **over) -> list[dict]:
    rec = {"boot_ok": True, "verdict": "PASS"}
    rec.update(over)
    return [dict(rec, skips=list(skips)) for _ in range(n)]


def test_below_the_floor_no_verdict():
    res = gate.analyse(_rows(9, ["[x] never runs"]), 25, 10)
    check("nine boots is not enough to accuse anything",
          (res["enough"], res["always"]), (False, []))


def test_at_the_floor_it_fires():
    res = gate.analyse(_rows(10, ["[x] never runs"]), 25, 10)
    check("ten boots, all skipping, is the finding",
          (res["enough"], res["always"]), (True, ["[x] never runs"]))


def test_one_boot_that_ran_it_clears_the_charge():
    rows = _rows(10, ["[x] never runs"]) + _rows(1, [])
    res = gate.analyse(rows, 25, 10)
    check("a single boot on which the section ran is a complete defence",
          res["always"], [])


def test_window_forgets_old_boots():
    # The fix landed; the next `window` boots are clean. The gate must stop
    # failing once the old rows fall out, or it teaches its reader to skip it.
    rows = _rows(30, ["[x] never runs"]) + _rows(25, [])
    res = gate.analyse(rows, 25, 10)
    check("a skip that stopped firing leaves the window", res["always"], [])


def test_rows_without_the_field_are_not_evidence():
    # A pre-field row has no opinion. Counting it as "did not skip" would break
    # every genuine offender's streak -- failure in the direction that hides.
    old = [{"boot_ok": True, "verdict": "PASS"} for _ in range(50)]
    res = gate.analyse(old + _rows(10, ["[x] never runs"]), 25, 10)
    check("rows predating the field neither count nor dilute",
          (res["n"], res["always"]), (10, ["[x] never runs"]))


def test_failed_boots_are_not_evidence():
    dead = _rows(50, [], boot_ok=False, verdict="TIMEOUT")
    res = gate.analyse(dead + _rows(10, ["[x] never runs"]), 25, 10)
    check("a boot that died early did not observe anything",
          (res["n"], res["always"]), (10, ["[x] never runs"]))


def test_experiments_are_not_evidence():
    probe = _rows(50, [], experiment="-cpu host")
    res = gate.analyse(probe + _rows(10, ["[x] never runs"]), 25, 10)
    check("a probe is evidence about the probe",
          (res["n"], res["always"]), (10, ["[x] never runs"]))


def test_allowlisted_skip_is_reported_not_failed():
    name = next(iter(gate.ALLOWED))
    res = gate.analyse(_rows(10, [name]), 25, 10)
    check("an allowlisted always-skip is a note, not a failure",
          (res["always"], res["allowed_firing"]), ([], [name]))


def test_stale_allowlist_entry_fails():
    # Every ALLOWED name absent from a 10-boot window is stale by definition.
    res = gate.analyse(_rows(10, []), 25, 10)
    check("an allowlist entry that stopped firing is a failure",
          res["stale_allowlist"], sorted(gate.ALLOWED))


HOTPLUG = "[hotplug] offline/online cycle"


def _boots(k: int, *, hotplug: bool, **over) -> list[dict]:
    """`k` boots skipping every unconditional allowlist entry, and the hotplug
    cycle when `hotplug`. With no `cpus`, a boot is single-CPU by the
    recorder's rule for a row without the field."""
    plain = [n for n, a in gate.ALLOWED.items() if a.only_on is None]
    return _rows(k, plain + ([HOTPLUG] if hotplug else []), **over)


def _single(k: int, *, hotplug: bool = True) -> list[dict]:
    return _boots(k, hotplug=hotplug)


def _multi(k: int, *, hotplug: bool, cpus=2) -> list[dict]:
    return _boots(k, hotplug=hotplug, cpus=cpus)


def _history(tmpdir: str, rows: list[dict]) -> str:
    path = os.path.join(tmpdir, "history.jsonl")
    with open(path, "w", encoding="utf-8", newline="") as handle:
        for rec in rows:
            handle.write(json.dumps(rec) + "\n")
    return path


def _main_says(argv: list[str]) -> tuple[int, str]:
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        rc = gate.main(argv)
    return rc, out.getvalue()


def test_conditional_entry_is_judged_on_its_own_boots():
    # On a boot given `--smp=2` the hotplug cycle runs. That is what the entry
    # says will happen, not evidence that it went stale.
    res = gate.analyse(_single(24) + _multi(1, hotplug=False), 25, 10)
    check("a two-CPU boot that ran the section leaves the entry standing",
          (HOTPLUG in res["allowed_firing"], res["stale_allowlist"],
           res["allowed_outside"], tuple(res["split"][HOTPLUG])),
          (True, [], [], (24, 24, 1, 0)))


def test_conditional_entry_must_fire_on_every_matching_boot():
    rows = _single(11) + _single(1, hotplug=False) + _multi(1, hotplug=False)
    res = gate.analyse(rows, 25, 10)
    check("one single-CPU boot that ran the section makes the entry false",
          res["stale_allowlist"], [HOTPLUG])


def test_no_matching_boot_in_the_window_is_stale():
    # The harness moved to two CPUs: the entry describes no boot being run.
    for fired in (False, True):
        res = gate.analyse(_multi(25, hotplug=fired), 25, 10)
        check(f"no single-CPU boot in the window, skip fired={fired}: stale",
              (res["stale_allowlist"], HOTPLUG in res["allowed_firing"]),
              ([HOTPLUG], False))


def test_outside_reaches_back_past_the_window():
    # Two-CPU boots are rare; a window of 25 rarely holds ten. Judged on the
    # last 25 of them in the whole history, ten that all skipped are found.
    res = gate.analyse(_multi(10, hotplug=True) + _single(25), 25, 10)
    check("ten two-CPU boots behind the window, all skipping: the condition "
          "explains nothing",
          (res["allowed_outside"], HOTPLUG in res["allowed_firing"],
           res["stale_allowlist"], tuple(res["split"][HOTPLUG])),
          ([HOTPLUG], False, [], (25, 25, 10, 10)))


def test_outside_is_its_own_window():
    ran_long_ago = (_multi(1, hotplug=False) + _multi(25, hotplug=True)
                    + _single(25))
    check("a run older than the last 25 two-CPU boots clears nothing",
          gate.analyse(ran_long_ago, 25, 10)["allowed_outside"], [HOTPLUG])
    ran_lately = (_multi(1, hotplug=False) + _multi(24, hotplug=True)
                  + _single(25))
    check("a run among the last 25 two-CPU boots clears the charge",
          gate.analyse(ran_lately, 25, 10)["allowed_outside"], [])


def test_outside_needs_the_floor_too():
    res = gate.analyse(_multi(9, hotplug=True) + _single(16), 25, 10)
    check("nine two-CPU boots are not enough to accuse the condition",
          (res["allowed_outside"], HOTPLUG in res["allowed_firing"]),
          ([], True))


def test_rows_that_cannot_say_are_in_neither_population():
    # A row recorded without its CPU count is no evidence either way.
    res = gate.analyse(_multi(10, hotplug=True, cpus=None) + _single(15),
                       25, 10)
    check("unknown-CPU rows count neither inside nor outside",
          (tuple(res["split"][HOTPLUG]), res["allowed_outside"],
           HOTPLUG in res["allowed_firing"]),
          ((15, 15, 0, 0), [], True))


def test_a_boot_outside_the_condition_is_not_refused(tmpdir):
    # Refusing the one boot that can record the fix would leave no way out.
    rows = _multi(10, hotplug=True) + _single(25)
    res = gate.analyse(rows, 25, 10, {"cpus": 2})
    check("a two-CPU boot about to run turns the failure into a note",
          (res["outside_pending"], res["allowed_outside"],
           res["stale_allowlist"]), ([HOTPLUG], [], []))
    path = _history(tmpdir, rows)
    check("main: refused before a one-CPU boot, and with no boot named, "
          "let through before a two-CPU one",
          [_main_says(["--history", path] + extra)[0]
           for extra in ([], ["--cpus", "1"], ["--cpus", "2"])],
          [1, 1, 0])


def test_single_cpu_follows_the_recorder():
    check("which rows count as single-CPU, and which cannot say",
          [gate.SINGLE_CPU.matches(rec) for rec in (
              {}, {"cpus": 1}, {"cpus": 2}, {"cpus": None},
              {"experiment": "-smp 4"})],
          [True, True, False, None, None])


def test_main_passes_with_a_two_cpu_boot_in_the_window(tmpdir):
    path = _history(tmpdir, _single(24) + _multi(1, hotplug=False))
    rc, said = _main_says(["--history", path])
    check("the window that refused lane A's boot on 2026-10-27 passes", rc, 0)
    check_true("...and the note gives both populations",
               f"{HOTPLUG} skipped on all 24 single-CPU boot(s) in the window "
               f"(and on 0 of the last 1 other boot(s))" in said)
    rc, listed = _main_says(["--history", path, "--list"])
    check_true("--list gives both populations too",
               f"{HOTPLUG} ALLOWED (single-CPU: 24/24, others: 0/1)" in listed)


def test_main_names_the_population_a_stale_entry_failed_on(tmpdir):
    rows = _single(11) + _single(1, hotplug=False) + _multi(1, hotplug=False)
    rc, said = _main_says(["--history", _history(tmpdir, rows)])
    check("stale is exit 1", rc, 1)
    check_true("...counted on the single-CPU boots",
               f"{HOTPLUG} is in ALLOWED but fired on only 11 of the 12 "
               f"single-CPU boot(s) in the window" in said)


def test_main_names_an_entry_whose_condition_describes_nothing(tmpdir):
    rc, said = _main_says(
        ["--history", _history(tmpdir, _multi(25, hotplug=False))])
    check("no single-CPU boot is exit 1", rc, 1)
    check_true("...and says so", "no boot in the window was one" in said)


def test_main_fails_a_condition_that_explains_nothing(tmpdir):
    path = _history(tmpdir, _multi(10, hotplug=True) + _single(10))
    rc, said = _main_says(["--history", path])
    check("never ran on either population is exit 1", rc, 1)
    check_true("...and the message counts the other boots",
               "skipped on all of the last 10 other boot(s)" in said)


def test_allowlist_entries_state_a_condition():
    # The docstring's rule, enforced: an entry that does not say what would
    # make the skip stop firing is the beginning of a dumping ground.
    bad = [k for k, v in gate.ALLOWED.items() if len(v.reason) < 60]
    check("every allowlist entry carries a real justification", bad, [])


def test_main_exits_zero_on_an_empty_history(tmpdir):
    path = os.path.join(tmpdir, "empty.jsonl")
    with open(path, "w", encoding="utf-8", newline="") as handle:
        handle.write("")
    check("an empty history is 'no verdict', not a failure",
          gate.main(["--history", path]), 0)


def test_main_survives_a_malformed_line(tmpdir):
    path = os.path.join(tmpdir, "bad.jsonl")
    with open(path, "w", encoding="utf-8", newline="") as handle:
        handle.write("{not json\n")
        for rec in _rows(10, ["[x] never runs"]):
            handle.write(json.dumps(rec) + "\n")
    check("one bad line does not blind the gate to the good ones",
          gate.main(["--history", path]), 1)


def test_main_missing_history_is_exit_two(tmpdir):
    check("an unreadable history is a tool failure, not a tree failure",
          gate.main(["--history", os.path.join(tmpdir, "nope.jsonl")]), 2)


# --------------------------------------------------------------------------

def main():
    tests = [(n, f) for n, f in sorted(globals().items())
             if n.startswith("test_") and callable(f)]
    if len(tests) < 30:
        print(f"FATAL: test discovery found only {len(tests)} tests; the suite "
              f"has at least 30. Discovery is broken, not the code.")
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
    print(f"all {len(tests)} check-boot-skips tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
