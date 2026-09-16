#!/usr/bin/env python3
r"""Break the code on purpose and report which tests noticed.

A test suite that is green proves nothing on its own: it is equally consistent
with code that works and with assertions that cannot fail. The cheapest way to
tell those apart is to introduce the defect the test claims to guard against
and watch it go red.

This is that loop, written once. It had been written by hand five times in a
day before it was a file, and the hand-written copies kept acquiring the same
two faults.

## The two faults, because they are the reason for the output format

**A sabotage that does not apply proves nothing, and looks like a pass.** If
the search text has drifted -- rustfmt moved a brace, a field was renamed --
the edit silently does not happen, the tests pass because nothing changed, and
a harness that only asks "did the expected test fail?" reports the same word it
would report for a genuine hole. Every sabotage here is required to match
exactly once, and a miss is `DID NOT APPLY`, never a pass.

**"Did not fail" and "is not there" are indistinguishable to a lookup.** A
harness that searches the output for a named test result and finds nothing
cannot tell "that test passed" from "that test does not exist". Mistype the
name and the tool reports the code unprotected while the assertion is in fact
firing. That happened: a correct app was nearly investigated for a hole in
`CANNOT_CONVERT_LINES` because the harness was looking for a test name that had
never existed.

The fix generalises past this file, and is the reason the format is what it is:
**print the set that actually went red, beside the verdict.** A count can be
printed for a corpus that came up empty; a lookup that finds nothing has no
count to print, so the set has to be shown instead. `nothing failed` and
`I found nothing` then stop looking alike.

## The third outcome, which is the interesting one

A sabotage can be caught by a test other than the one predicted. That is not a
pass and not a failure -- it is **coverage that exists where you did not think
it did**, and it is worth its own word (`PREDICTION`) because it has twice now
led straight to a vacuous assertion in the test that was expected to catch it.
The most recent: a row-status assertion whose fixture began at the exact value
the sabotage writes, so "the state did not change" was satisfied by the defect
as readily as by the fix.

## Usage

    python scripts/sabotage.py plan.json
    python scripts/sabotage.py --self-test

    {
      "file": "apps/sysmonitor/src/main.rs",
      "package": "sysmonitor",
      "sabotages": [
        {"name": "the flag is dropped",
         "old": "asserted: irq.pending,",
         "new": "asserted: false,",
         "expect": ["the_interrupt_lines_are_read"]}
      ]
    }

The file is restored from the bytes read at the start and the restore is
verified by SHA-256 before exit, including on exception.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import selftestflag  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"


def red_tests(out: str) -> set[str]:
    """Names of tests that FAILED, as cargo's per-test lines report them.

    The module path is dropped so a plan can name `the_door_opens` without
    knowing whether it lives in `tests::` or `ui::tests::`.
    """
    red = set()
    for line in out.splitlines():
        # `test result: FAILED. 76 passed; 2 failed` begins with "test " and
        # contains "FAILED", so the obvious predicate harvests a test named
        # `result:`. Every hand-written copy of this had that bug; it showed up
        # in their output as a stray `'result:'` beside the real names and got
        # skimmed past five times. The `...` is what separates a per-test line
        # from the summary.
        if not line.startswith("test ") or line.startswith("test result:"):
            continue
        parts = line.split()
        if len(parts) >= 4 and parts[2] == "..." and parts[3].startswith("FAILED"):
            red.add(parts[1].split("::")[-1])
    return red


def classify(applied: bool, broke: bool, red: set[str], expect: list[str]) -> tuple[str, str]:
    """(verdict, why) for one sabotage run.

    Split out from the running so it can be tested without a cargo project --
    the same reason `signal_outcome` is split from `kill` in the apps this was
    written for. A harness whose own classification is untested is the thing it
    exists to prevent.
    """
    if not applied:
        return "DID-NOT-APPLY", "the search text did not match exactly once"
    if broke:
        # A compile error shows the compiler rejected the edit, not that any
        # assertion noticed anything.
        return "NOT-EVIDENCE", "the sabotaged tree did not compile"
    hit = [t for t in expect if t in red]
    if hit:
        return "CAUGHT", f"{', '.join(hit)} went red"
    if red:
        return "PREDICTION", (
            f"caught by {', '.join(sorted(red))}, not by the expected "
            f"{', '.join(expect)} -- check whether the expected one can fail at all"
        )
    return "HOLE", "nothing went red"


MARK = {
    "CAUGHT": "ok  ",
    "PREDICTION": "??  ",
    "HOLE": "!!  ",
    "DID-NOT-APPLY": "!!  ",
    "NOT-EVIDENCE": "--  ",
}


def run(plan: dict) -> int:
    f = ROOT / plan["file"]
    package = plan["package"]
    original = f.read_bytes()
    before = hashlib.sha256(original).hexdigest()

    results = []
    try:
        for sab in plan["sabotages"]:
            src = f.read_text(encoding="utf-8")
            applied = src.count(sab["old"]) == 1
            broke, red = False, set()
            if applied:
                f.write_text(src.replace(sab["old"], sab["new"], 1),
                             encoding="utf-8", newline="\n")
                r = subprocess.run(
                    [sys.executable, str(ROOT / "scripts" / "run-timeout.py"), "600",
                     "cargo", "test", "-p", package, "--target", TARGET],
                    capture_output=True, text=True, errors="replace", cwd=ROOT,
                )
                out = r.stdout + r.stderr
                broke = "error[E" in out or "error: could not compile" in out
                red = red_tests(out)
                f.write_bytes(original)
            verdict, why = classify(applied, broke, red, sab.get("expect", []))
            results.append((verdict, sab["name"]))
            # The red set is printed every time, including when it is empty,
            # because that is the whole point: an empty set is a fact about
            # this run and not a word that resembles a pass.
            print(f"{MARK[verdict]}{sab['name']}: {why}")
            print(f"      red: {sorted(red) or 'none'}")
    finally:
        f.write_bytes(original)
        if hashlib.sha256(f.read_bytes()).hexdigest() != before:
            print("RESTORE FAILED -- the file on disk is not what it was",
                  file=sys.stderr)
            return 2
        print("restore verified by hash")

    bad = [n for v, n in results if v in ("HOLE", "DID-NOT-APPLY")]
    unsure = [n for v, n in results if v in ("PREDICTION", "NOT-EVIDENCE")]
    print()
    print(f"{len(results)} sabotage(s): "
          f"{sum(1 for v, _ in results if v == 'CAUGHT')} caught, "
          f"{len(unsure)} inconclusive, {len(bad)} uncovered")
    if unsure:
        print(f"  inconclusive: {'; '.join(unsure)}")
    if bad:
        print(f"  UNCOVERED:    {'; '.join(bad)}")
    return 1 if bad else 0


def _self_test() -> int:
    failures = 0

    def expect(label, got, want):
        nonlocal failures
        if got != want:
            failures += 1
            print(f"FAIL  {label}\n  got  {got!r}\n  want {want!r}")
        else:
            print(f"  ok    {label}")

    expect("a failed test named in expect is CAUGHT",
           classify(True, False, {"the_door_opens"}, ["the_door_opens"])[0], "CAUGHT")
    expect("nothing red at all is a HOLE",
           classify(True, False, set(), ["the_door_opens"])[0], "HOLE")
    # The one this file exists for. A typo'd name used to render as a HOLE and
    # send someone looking for a defect in working code.
    expect("a different test catching it is PREDICTION, not HOLE",
           classify(True, False, {"some_other_test"}, ["the_door_opns"])[0], "PREDICTION")
    expect("an edit that did not apply proves nothing",
           classify(False, False, set(), ["x"])[0], "DID-NOT-APPLY")
    expect("...even when something else happens to be red",
           classify(False, False, {"unrelated"}, ["x"])[0], "DID-NOT-APPLY")
    expect("a broken build is not evidence",
           classify(True, True, set(), ["x"])[0], "NOT-EVIDENCE")
    expect("...and stays not-evidence even if tests went red",
           classify(True, True, {"x"}, ["x"])[0], "NOT-EVIDENCE")

    # Cargo's real output shapes, verbatim.
    out = (
        "test tests::the_door_opens ... FAILED\n"
        "test tests::the_door_shuts ... ok\n"
        "test ui::tests::a_nested_one ... FAILED\n"
        "test result: FAILED. 76 passed; 2 failed; 0 ignored\n"
    )
    expect("module paths are dropped so a plan need not know them",
           red_tests(out), {"the_door_opens", "a_nested_one"})
    expect("the summary line is not a test name",
           "result:" in red_tests(out), False)
    expect("a passing test is not red", "the_door_shuts" in red_tests(out), False)

    print(f"sabotage: self-test {'passed' if not failures else 'FAILED'} "
          f"({failures} failure(s))")
    return 1 if failures else 0


def main() -> int:
    argv = sys.argv[1:]
    if selftestflag.wants_selftest(argv):
        return _self_test()
    bad = [a for a in selftestflag.unknown_options(argv)]
    if bad:
        print(f"unrecognised option(s): {', '.join(bad)}", file=sys.stderr)
        return 2
    if len(argv) != 1:
        print("usage: python scripts/sabotage.py <plan.json>", file=sys.stderr)
        return 2
    plan = json.loads(Path(argv[0]).read_text(encoding="utf-8"))
    return run(plan)


if __name__ == "__main__":
    raise SystemExit(main())
