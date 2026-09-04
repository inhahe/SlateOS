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
trusted to mean anything (see `the gate is never written to`).

## The gate is never written to

Each mutant is written to a **sibling file** -- `.mutant-<pid>-<gate>` in the
same directory -- and that copy is what gets run. The gate itself is only ever
read.

The first version did the obvious thing instead: mutate in place, restore in a
`finally`. It works right up until something skips the `finally`. A SIGKILL or
a machine going down leaves the gate mutated on disk, and the next sweep then
issues verdicts about a program that was already broken somewhere else -- all
of which look perfectly normal. Worse, two sweeps at once means the second
one's "original" is the first one's mutant; that is not hypothetical, it
happened while this file was being written.

Both were caught by a guard, and a guard against a hazard is worth less than
not having the hazard. Writing to a sibling removes the failure mode rather
than detecting it: nothing to restore, so nothing to fail to restore, and the
pid in the name means two sweeps cannot collide. It is also what makes the
sweep safe to run unattended in a tree three lanes share.

The sibling has to live in the gate's own directory, not in a temp dir,
because a gate locates the tree relative to its own file --
`check-doc-links.py` computes `ROOT` as its parent's parent -- and a copy
somewhere else would scan somewhere else.

## Still checking the clean run first

One thing the copy does not remove: a gate whose self-test genuinely fails
kills every mutant, because every mutant's run fails too, and the sweep
reports a perfect score for a suite that is asserting nothing. So the clean
run still happens before any mutation, and failing it is still exit 2. It is
no longer a check for a dirty tree; it is a check that the suite works at all.
"""

import ast
import os
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

    Returning None is not "no such flag" -- it is the abort. A gate whose suite
    does not pass before anything has been done to it kills every mutant for
    the wrong reason: the mutant's run fails because the *clean* run fails, and
    the sweep reports a perfect score for a suite that is asserting nothing.
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


def run_mutants(gate: Path, source: str, table: list[tuple[str, str, str]],
                flag: str, say=None) -> list[str]:
    """Every row that did not die, as a line naming why, given a live `flag`.

    `gate` is opened for reading and never for writing: each mutant goes to a
    sibling file whose name carries this process's pid, so a killed run leaves
    at most one stray copy and two concurrent sweeps cannot see each other's.
    The sibling must be a sibling and not a temp file elsewhere, because a gate
    finds the tree relative to its own path.
    """
    say = say or (lambda _line: None)

    # The gate's code, with its own mutation table cut out -- see `table_span`.
    lo, hi = table_span(source)
    head, body, tail = source[:lo], source[lo:hi], source[hi:]

    # Not survivors and not kills: the sweep could not ask the question at all.
    # Counted as failures anyway, because a needle that matches nothing is a
    # hole that reads as coverage.
    problems = needle_problems(source, table)
    dead = {i for i, _, _ in problems}
    survivors: list[str] = []
    for _, label, reason in problems:
        say(f"[BAD NEEDLE] {label}: {reason}")
        survivors.append(f"{label} ({reason})")

    mutant = gate.with_name(f".mutant-{os.getpid()}-{gate.name}")
    try:
        for i, (label, old, new) in enumerate(table):
            if i in dead:
                continue
            mutant.write_text(
                head.replace(old, new) + body + tail.replace(old, new),
                encoding="utf-8")
            if selftest_passes(mutant, flag):
                say(f"[SURVIVED] {label}")
                survivors.append(label)
            else:
                say(f"[killed]   {label}")
    finally:
        mutant.unlink(missing_ok=True)
    return survivors


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
              f"       or its suite is genuinely failing right now.\n"
              f"       Every mutant would then die of the clean run's failure "
              f"and score as a kill.")
        return 2

    survivors = run_mutants(gate, original, table, flag, say=print)

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
