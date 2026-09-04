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
import runpy
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


def selftest_passes(gate: Path) -> bool:
    """Whether `gate --selftest` exits 0, with its output discarded."""
    r = subprocess.run([sys.executable, str(gate), "--selftest"],
                       capture_output=True, text=True)
    return r.returncode == 0


def load_table(gate: Path) -> list[tuple[str, str, str]] | None:
    """The gate's `SELFTEST_MUTANTS`, or None if it declares none.

    `runpy` rather than `import`: a gate is named `check-doc-links.py`, and a
    hyphen is not a legal module name. Executing the module is safe because
    every gate here puts its work behind `if __name__ == "__main__"` -- and
    `runpy.run_path` sets `__name__` to something else, so nothing runs.
    """
    ns = runpy.run_path(str(gate))
    table = ns.get("SELFTEST_MUTANTS")
    if table is None:
        return None
    return [tuple(row) for row in table]


def sweep(gate: Path) -> int:
    original = gate.read_text(encoding="utf-8")

    table = load_table(gate)
    if table is None:
        print(f"{gate.name}: declares no SELFTEST_MUTANTS -- nothing to sweep.")
        return 0

    if not selftest_passes(gate):
        print(f"ABORT: {gate.name} --selftest does not pass before any mutation "
              f"was applied.\n"
              f"       Either the gate is genuinely broken, or a previous sweep "
              f"died without restoring it,\n"
              f"       or another sweep is running right now. Every verdict "
              f"below would describe some other program.")
        return 2

    # The gate's code, with its own mutation table cut out -- see `table_span`.
    lo, hi = table_span(original)
    head, body, tail = original[:lo], original[lo:hi], original[hi:]

    survivors: list[str] = []
    try:
        for label, old, new in table:
            n = head.count(old) + tail.count(old)
            if n != 1:
                # Not a survivor and not a kill: the sweep could not ask the
                # question. Counted as a failure anyway, because a needle that
                # matches nothing is a hole that reads as coverage.
                print(f"[BAD NEEDLE] {label}: {n} occurrence(s) of {old!r}")
                survivors.append(f"{label} (needle matches {n} times, want 1)")
                continue
            mutated = head.replace(old, new) + body + tail.replace(old, new)
            if mutated == original:
                print(f"[BAD NEEDLE] {label}: replacement changes nothing")
                survivors.append(f"{label} (no-op mutant)")
                continue
            gate.write_text(mutated, encoding="utf-8")
            if selftest_passes(gate):
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
