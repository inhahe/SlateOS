"""Break one piece of a Python gate at a time and demand its `--selftest` notice.

## What this is for

A gate that exits 0 and a gate that cannot exit anything else are spelled
identically. `check-gates-can-refuse.py` answers that at the coarsest level --
can this gate reach a non-zero exit at all -- and `--selftest` answers it per
case. Neither answers the question in between: **is each individual guard
inside the gate load-bearing, or is it decoration?**

That question is not academic here. Twice now the answer has been "decoration":

* `check-libc-shape.py` pinned `MIN_MEMBERS`/`MIN_SYMBOLS` against a measured
  archive, and `main()` never read them.
* `check-doc-links.py` grew a five-part coverage floor whose fixtures were
  derived from the constants under test, so setting three of those constants
  to zero shrank the fixtures to match and all 74 cases stayed green.

Both were found by gutting a constant and seeing whether anything objected.
Nothing else finds them: a green suite is the symptom, not the diagnosis.

## Why the table lives in the gate

The obvious place for the mutation table is a script beside the gate, and that
is where the first two lived -- one throwaway per gate, thrown away after use,
which is `known-issues.md` lesson 63 (a rule kept only by copying is a rule
that will be dropped) waiting to happen a third time.

So the harness is here, once, and each gate carries its own table as a
module-level `SELFTEST_MUTANTS`. That is not merely tidier: a mutation needle
is a *quotation* of the gate's source, and a quotation kept in another file
goes stale silently the moment the line it quotes is reworded. A stale needle
does not fail -- it matches nothing, gets skipped, and leaves a hole that reads
as coverage. Keeping the two in one file means the rename that breaks the
needle is in the same diff as the needle.

## Using it

In the gate, near the code it breaks:

    SELFTEST_MUTANTS = [
        ("what the mutant pretends is true", "<exact source text>", "<replacement>"),
        ...
    ]

Then:

    python scripts/mutate-gate.py scripts/check-doc-links.py

Exit 0 if every mutant died, 1 if any survived, 2 if the sweep could not be
trusted to mean anything (see `refuse a dirty start`).

## Refusing a dirty start

The sweep mutates the gate in place and restores it in a `finally`. Two things
defeat that, and both are checked before any mutation:

* A run killed by something that never asks for cleanup -- a SIGKILL, the
  machine going down -- leaves the gate mutated on disk. Mutating on top of
  that produces verdicts about a program already broken somewhere else, and
  they all look normal.
* Two sweeps at once. The second one's "original" is the first one's mutant.
  This is not hypothetical; it happened while developing this file, and the
  guard below is what caught it.

Both show up the same way: the gate's own `--selftest` does not pass before
anything has been done to it. That is the check.
"""

import ast
import subprocess
import sys
from pathlib import Path


def table_span(source: str) -> tuple[int, int]:
    """The half-open character range of the `SELFTEST_MUTANTS` assignment.

    The table has to be excluded from the search, and this is why: every needle
    is a quotation of the gate, so the moment the table moved *into* the gate
    each needle matched twice -- once in the code and once in its own row. The
    harness reported seventeen ambiguous needles and swept nothing.

    The alternative was to anchor every needle with surrounding newlines so it
    could not match inside a quoted string. That works, but it makes a correct
    table depend on the author remembering an invisible convention, and the
    failure is a needle that quietly matches the wrong thing. Cutting the table
    out by parsing is exact, and it is checked: `ast` reports the assignment's
    real extent, not a guess from a marker comment that a reformat could move.
    """
    tree = ast.parse(source)
    for node in tree.body:
        targets = getattr(node, "targets", [])
        if not isinstance(node, ast.Assign) or len(targets) != 1:
            continue
        t = targets[0]
        if isinstance(t, ast.Name) and t.id == "SELFTEST_MUTANTS":
            lines = source.splitlines(keepends=True)
            start = sum(len(x) for x in lines[: node.lineno - 1])
            end = sum(len(x) for x in lines[: node.end_lineno])
            return start, end
    return 0, 0


#: Both spellings in use. `check-doc-links.py` says `--selftest`,
#: `check-libc-shape.py` says `--self-test`, and each is published -- named in
#: `boot-test.sh` and read by `check-gates-are-wired.py` -- so neither can be
#: renamed to suit this file. The flag is discovered instead of assumed, and
#: the discovery is free: the sweep already has to run the suite once before
#: mutating anything, so that run is what identifies the spelling.
SELFTEST_FLAGS = ("--selftest", "--self-test")


def selftest_passes(gate: Path, flag: str) -> bool:
    """Whether `gate <flag>` exits 0, with its output discarded."""
    r = subprocess.run([sys.executable, str(gate), flag],
                       capture_output=True, text=True)
    return r.returncode == 0


def find_selftest_flag(gate: Path) -> str | None:
    """The spelling this gate answers to, or None if it passes under neither.

    Returning None is not "no such flag" -- it is the dirty-start abort. A gate
    whose suite does not pass before anything has been done to it cannot give a
    meaningful verdict about a mutant, whether the cause is a genuine failure,
    a sweep killed before its restore, or a second sweep running right now.
    """
    for flag in SELFTEST_FLAGS:
        if selftest_passes(gate, flag):
            return flag
    return None


class BadTable(Exception):
    """The `SELFTEST_MUTANTS` assignment exists but cannot be read as a table."""


def table_from_source(source: str) -> list[tuple[str, str, str]] | None:
    """The `SELFTEST_MUTANTS` a gate's source declares, or None if it declares none.

    Read as a *literal*, not by executing the gate. The first version ran
    `runpy.run_path` -- `import` is unavailable because a gate is named
    `check-doc-links.py` and a hyphen is not a legal module name -- and that
    works, but it makes reading the table cost whatever importing the gate
    costs, which is the wrong price for `check-mutation-needles.py` to pay on
    every boot test just to find out what the rows say.

    Requiring a literal is a rule worth having on its own: a table assembled at
    runtime is one this file and that gate could read differently, and two
    readings of the same table is precisely the failure the table exists to
    prevent. `ast.literal_eval` refusing is therefore a finding, not a reason
    to fall back to executing anything.
    """
    tree = ast.parse(source)
    for node in tree.body:
        targets = getattr(node, "targets", [])
        if not isinstance(node, ast.Assign) or len(targets) != 1:
            continue
        t = targets[0]
        if isinstance(t, ast.Name) and t.id == "SELFTEST_MUTANTS":
            try:
                value = ast.literal_eval(node.value)
            except (ValueError, SyntaxError) as exc:
                raise BadTable(f"not a literal ({exc})") from exc
            if not isinstance(value, list):
                raise BadTable(f"is a {type(value).__name__}, want a list")
            rows = []
            for i, row in enumerate(value):
                if (not isinstance(row, tuple) or len(row) != 3
                        or not all(isinstance(x, str) for x in row)):
                    raise BadTable(
                        f"row {i} is not a 3-tuple of strings: {row!r}")
                rows.append(row)
            return rows
    return None


def needle_problems(
        source: str,
        table: list[tuple[str, str, str]]) -> list[tuple[int, str, str]]:
    """Every reason a row could not ask its question, as `(row, label, reason)`.

    Keyed by row index rather than by label because two rows may legitimately
    carry the same label, and skipping a good row because a namesake was bad
    would be the same silent hole this function exists to report.

    This is the half of the sweep that needs no mutation to run, and it is
    split out because it is the half that can be automated. The sweep itself
    rewrites the gate on disk, so it can never be wired into a boot test in a
    tree three lanes share -- the dirty-start guard exists because two sweeps
    at once already happened once. A stale needle, though, is exactly as fatal
    and entirely static: it matches nothing, gets skipped, and leaves a hole
    that reads as coverage. `check-mutation-needles.py` runs this on every
    boot test for that reason.
    """
    lo, hi = table_span(source)
    head, tail = source[:lo], source[hi:]
    problems = []
    for i, (label, old, new) in enumerate(table):
        n = head.count(old) + tail.count(old)
        if n != 1:
            problems.append(
                (i, label,
                 f"needle matches {n} time(s) outside the table, want 1"))
        elif old == new:
            problems.append((i, label, "replacement is identical to the needle"))
    return problems


def sweep(gate: Path) -> int:
    original = gate.read_text(encoding="utf-8")

    try:
        table = table_from_source(original)
    except BadTable as exc:
        print(f"ABORT: {gate.name}'s SELFTEST_MUTANTS {exc}.")
        return 2
    if table is None:
        print(f"{gate.name}: declares no SELFTEST_MUTANTS -- nothing to sweep.")
        return 0

    flag = find_selftest_flag(gate)
    if flag is None:
        print(f"ABORT: {gate.name} passes under neither "
              f"{' nor '.join(SELFTEST_FLAGS)}, before any mutation was "
              f"applied.\n"
              f"       Either the gate is genuinely broken, or it declares a "
              f"table but has no self-test,\n"
              f"       or a previous sweep died without restoring it, or "
              f"another sweep is running right now.\n"
              f"       Every verdict below would describe some other program.")
        return 2

    # The gate's code, with its own mutation table cut out -- see `table_span`.
    lo, hi = table_span(original)
    head, body, tail = original[:lo], original[lo:hi], original[hi:]

    # Not survivors and not kills: the sweep could not ask the question at all.
    # Counted as failures anyway, because a needle that matches nothing is a
    # hole that reads as coverage.
    problems = needle_problems(original, table)
    dead = {i for i, _, _ in problems}
    survivors: list[str] = []
    for _, label, reason in problems:
        print(f"[BAD NEEDLE] {label}: {reason}")
        survivors.append(f"{label} ({reason})")

    try:
        for i, (label, old, new) in enumerate(table):
            if i in dead:
                continue
            mutated = head.replace(old, new) + body + tail.replace(old, new)
            gate.write_text(mutated, encoding="utf-8")
            if selftest_passes(gate, flag):
                print(f"[SURVIVED] {label}")
                survivors.append(label)
            else:
                print(f"[killed]   {label}")
    finally:
        gate.write_text(original, encoding="utf-8")

    killed = len(table) - len(survivors)
    print(f"\n{killed}/{len(table)} mutants killed")
    if survivors:
        print("\nSurvivors. Each is a hole in the self-test, not a weak mutant:")
        for s in survivors:
            print(f"  - {s}")
        return 1
    return 0


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__)
        return 2
    gate = Path(argv[1]).resolve()
    if not gate.is_file():
        print(f"no such gate: {gate}", file=sys.stderr)
        return 2
    return sweep(gate)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
