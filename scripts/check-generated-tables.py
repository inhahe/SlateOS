#!/usr/bin/env python3
r"""Verify that checked-in generated tables still match what their generator emits.

Run this after every merge, next to ``scripts/ki_dupes.py``::

    python scripts/check-generated-tables.py

Exit 0 = every table matches, 1 = a table has drifted from its generator,
2 = could not verify (which is a failure, not a skip).

The check is **read-only**: each table is regenerated into a scratch copy of the
tree and the original bytes are put back before the script returns, so a run
never leaves the working tree dirty -- including on drift, and including if it
is interrupted.

---------------------------------------------------------------------------
The gap this fills, and why the two sibling checks do not fill it
---------------------------------------------------------------------------
``gui/font/src/*_machine.rs`` are DFA transition tables -- ``indic_machine.rs``
alone is 127 states over 34 categories -- emitted by scripts in
``gui/font/tools/``. Nothing checked that a table still matches its generator.
They are the least reviewable files in the tree: a wrong row does not fail to
compile, does not fail a unit test that does not know the right answer, and
surfaces only as a shaping difference in one script that nobody traces back to
a table. That is exactly the profile that wants a mechanical check.

  * **``ctest-fixtures.py``-style content stamps** would work, but there is
    nothing to stamp *against*: the stamp records a hash of the inputs, and the
    input here is the generator, which is already tracked. Hashing a tracked
    file against a tracked file is what ``git diff`` does.
  * **An ancestry check** asks a question about *history* -- "did a source
    commit land after the artifact's commit?" -- and for this family that
    question false-positives, which was established rather than assumed.
    ``9b75e15aa`` edited ``gen_indic_machine.py`` after ``indic_machine.rs`` was
    last written, so an ancestry check flags it. The edit was
    ``compile_rules(rules)`` -> ``compile_rules(rules, categories=CATEGORIES)``
    so the Universal Shaping Engine could reuse the machinery: the default is
    the old constant, and regenerating produces a byte-identical file. A history
    check cannot tell that refactor from a real one, and the script that gets
    flagged for a non-problem is the script that gets ignored. (``scripts/
    stamp-ancestry.py`` was the tree's one ancestry check; it was retired in
    design-decisions.md §277 for a related reason -- it ended up flagging
    *everything* -- so this bullet now argues against a technique rather than
    against an existing script.)

So this asks the question directly: **run the generator, diff the output.** No
false positives are possible, because the answer is the artifact itself.

---------------------------------------------------------------------------
Why only four of the fifteen generators are listed
---------------------------------------------------------------------------
Eleven of ``gui/font/tools/gen_*.py`` read the Unicode Character Database, or
HarfBuzz's sources, from outside the repository. Those are not reproducible
offline, and a check that needs a download is a check that gets skipped -- so
they are deliberately *not* listed, and this script makes no claim about them.
Their protection is the Unicode version recorded in each file's header, plus
the oracle diffs their generators run at generation time against HarfBuzz.

The four listed here need nothing but Python: they are pure constructions
(Thompson, subset, Moore) over a grammar transcribed into the generator itself.
They are also the four whose output is least reviewable by eye, which is the
happy case where the cheapest check covers the highest-value files.

**A listed table whose generator is missing is an error (exit 2), not a skip.**
A rename that silently disarmed a row would leave the check green and useless,
which is ``B-PATHZ-PREREQUISITE-SKIPS-ARE-SILENT`` again.
"""

from __future__ import annotations

import pathlib
import subprocess
import sys
from dataclasses import dataclass

REPO = pathlib.Path(__file__).resolve().parent.parent


@dataclass(frozen=True)
class Generated:
    """One checked-in file and the script that emits it."""

    #: Path of the generated file, relative to the repository root.
    table: str
    #: Path of the generator, relative to the repository root. It is expected to
    #: write `table` itself when run with no arguments.
    generator: str
    why: str


TABLES: tuple[Generated, ...] = (
    Generated(
        table="gui/font/src/indic_machine.rs",
        generator="gui/font/tools/gen_indic_machine.py",
        why="127-state DFA over 34 Indic categories",
    ),
    Generated(
        table="gui/font/src/khmer_machine.rs",
        generator="gui/font/tools/gen_khmer_machine.py",
        why="Khmer cluster DFA, same machinery as Indic",
    ),
    Generated(
        table="gui/font/src/myanmar_machine.rs",
        generator="gui/font/tools/gen_myanmar_machine.py",
        why="Myanmar cluster DFA, same machinery as Indic",
    ),
    Generated(
        table="gui/font/src/universal_machine.rs",
        generator="gui/font/tools/gen_universal_machine.py",
        why="110-state USE cluster DFA over 44 categories",
    ),
)

TAG = "[gentables]"


def log(msg: str) -> None:
    print(f"{TAG} {msg}")


def check(entry: Generated) -> str:
    """Regenerate one table and compare. Returns "ok", "drift" or "error".

    The table's original bytes are restored before returning in every case,
    including when the generator raises, so the caller's tree is untouched.
    """
    table = REPO / entry.table
    generator = REPO / entry.generator

    if not generator.is_file():
        log(f"ERROR {entry.generator} does not exist -- the row is disarmed, not passing")
        return "error"
    if not table.is_file():
        log(f"ERROR {entry.table} does not exist but is listed as generated")
        return "error"

    original = table.read_bytes()
    try:
        proc = subprocess.run(
            (sys.executable, str(generator)),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            cwd=REPO,
        )
        if proc.returncode != 0:
            log(f"ERROR {entry.generator} exited {proc.returncode}:")
            for line in (proc.stderr or proc.stdout).strip().splitlines()[-8:]:
                log(f"      {line}")
            return "error"
        produced = table.read_bytes()
    finally:
        # Unconditional: a drift report must not also be a working-tree edit,
        # and a generator that crashed halfway may have truncated the file.
        table.write_bytes(original)

    if produced == original:
        return "ok"

    log(f"DRIFT {entry.table} is not what {entry.generator} produces")
    log(f"      ({entry.why})")
    log("      The checked-in table was left untouched. To adopt the new one:")
    log(f"        python {entry.generator}")
    log("      Then read the diff before committing: a table changing when you")
    log("      did not mean to change one is the finding, not a formality.")
    return "drift"


def main() -> int:
    results = [check(entry) for entry in TABLES]

    if "error" in results:
        log("could not verify every table -- treating that as a failure")
        return 2
    if "drift" in results:
        return 1
    log(f"ok: all {len(TABLES)} generated tables match their generators")
    return 0


if __name__ == "__main__":
    sys.exit(main())
