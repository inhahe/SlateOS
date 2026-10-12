#!/usr/bin/env python3
r"""Verify that checked-in generated tables still match what their generator emits.

Run this after every merge::

    python scripts/check-generated-tables.py
    python scripts/check-generated-tables.py --self-test

Exit 0 = every table matches; 1 = a table has drifted from its generator, or
could not be verified -- a generator that will not run, or a listed file that
is missing (see `run` for why that is 1 and not 2); 2 = an option this script
does not know, which runs nothing.

The check is **read-only**: each generator rewrites its table in place, and the
table's original bytes are put back before the check of it returns, so a run
never leaves the working tree dirty -- including on drift, a generator that
fails halfway, and an interruption (the restore is in a `finally`). Only a
process killed outright, with no chance to run it, could leave a table
regenerated; `git diff` would show it.

``--self-test`` checks those promises against fixture generators and tables in
a temporary directory: a match passes, one changed row is refused and named,
a generator that fails or is missing is refused as unverifiable, and in every
case -- an interruption included -- the table's bytes are what they were.

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

import contextlib
import io
import pathlib
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from typing import Callable

import selftestflag

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


def run_generator(generator: pathlib.Path, root: pathlib.Path) -> subprocess.CompletedProcess:
    """Run one generator, with no arguments, from `root`."""
    return subprocess.run(
        (sys.executable, str(generator)),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        cwd=root,
    )


def check(
    entry: Generated,
    root: pathlib.Path = REPO,
    runner: Callable[[pathlib.Path, pathlib.Path], subprocess.CompletedProcess] = run_generator,
) -> str:
    """Regenerate one table under `root` and compare. Returns "ok", "drift" or
    "error".

    The table's original bytes are restored before returning in every case,
    including when the generator raises, so the caller's tree is untouched.
    `runner` runs the generator; the self-test replaces it to interrupt one.
    """
    table = root / entry.table
    generator = root / entry.generator

    if not generator.is_file():
        log(f"ERROR {entry.generator} does not exist -- the row is disarmed, not passing")
        return "error"
    if not table.is_file():
        log(f"ERROR {entry.table} does not exist but is listed as generated")
        return "error"

    original = table.read_bytes()
    try:
        proc = runner(generator, root)
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


def run(tables: tuple[Generated, ...], root: pathlib.Path = REPO) -> int:
    """Check every table in `tables` under `root`; the exit code."""
    results = [check(entry, root) for entry in tables]

    if "error" in results:
        # 1, not 2, and the distinction is the whole point of the two codes.
        # 2 means "could not look" -- the gate ran, reached no judgement, and
        # must not be read as having reached a clean one. That is the right
        # answer for a gate grading something another lane may never have
        # built, which is where the convention came from
        # (`check-libc-shape.py`, design-decisions.md S747).
        #
        # This is not that. Every generator here is a checked-in script in
        # this repository, run against a checked-in table. If one will not
        # run, the tool is broken *now*, for everybody, and the tables it
        # backs are unverifiable until someone fixes it. There is nothing to
        # wait for and nobody else to attribute it to, so it blocks.
        #
        # The comment below has always said "treating that as a failure";
        # until 2026-09-03 the return value quietly stopped agreeing with it,
        # because a binary runner had left 2 as the only non-green code worth
        # reaching for. Asked by lane B in
        # requests/b-c-check-generated-tables-returns-2-which-now-means-no-verdict.md.
        log("could not verify every table -- treating that as a failure")
        return 1
    if "drift" in results:
        return 1
    log(f"ok: all {len(tables)} generated tables match their generators")
    return 0


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------

#: The fixture table's rows, as its fixture generator writes them.
FIXTURE_ROWS = ("const T: [u8; 3] = [\n", "    1, 2, 3,\n", "];\n")

#: A generator writing the fixture table, as the real ones write theirs: the
#: table, in place, from the directory it is run in.
FIXTURE_GENERATOR = (
    "import pathlib\n"
    "pathlib.Path('gen/table.rs').write_text({rows!r}, encoding='utf-8', newline='\\n')\n"
)

#: A generator that truncates its table and then fails: what a crash halfway
#: through looks like from outside.
FAILING_GENERATOR = (
    "import pathlib, sys\n"
    "pathlib.Path('gen/table.rs').write_text('', encoding='utf-8')\n"
    "sys.exit(3)\n"
)


def self_test() -> int:
    """Check the promises in the module docstring against fixtures; 0 if
    every verdict is right."""
    cases = 0
    failures = 0

    def expect(ok: bool, what: str) -> None:
        nonlocal cases, failures
        cases += 1
        if not ok:
            failures += 1
            print(f"{TAG} SELF-TEST FAIL: {what}")

    entry = Generated(table="gen/table.rs", generator="gen/gen_table.py", why="fixture")
    good = "".join(FIXTURE_ROWS).encode("utf-8")
    drifted = good.replace(b"1, 2, 3", b"1, 9, 3")

    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        (root / "gen").mkdir()
        table = root / entry.table
        generator = root / entry.generator

        def setup(table_bytes: bytes | None, generator_source: str | None) -> None:
            if table_bytes is None:
                table.unlink(missing_ok=True)
            else:
                table.write_bytes(table_bytes)
            if generator_source is None:
                generator.unlink(missing_ok=True)
            else:
                generator.write_text(generator_source, encoding="utf-8", newline="")

        good_generator = FIXTURE_GENERATOR.format(rows="".join(FIXTURE_ROWS))

        # A table its generator emits byte for byte: passed.
        setup(good, good_generator)
        expect(check(entry, root) == "ok", "a matching table was not passed")
        expect(table.read_bytes() == good, "a matching table was changed")
        expect(run((entry,), root) == 0, "a matching table did not exit 0")

        # One row changed: refused, naming the table, and the drifted bytes
        # kept -- the check reports the drift, it does not repair it.
        setup(drifted, good_generator)
        expect(check(entry, root) == "drift", "a changed row was not refused as drift")
        expect(table.read_bytes() == drifted, "a drifted table was not restored")
        said = io.StringIO()
        with contextlib.redirect_stdout(said):
            code = run((entry,), root)
        expect(code == 1, "a drifted table did not exit 1")
        expect(f"DRIFT {entry.table}" in said.getvalue(), "the drift did not name the table")

        # A generator that truncates the table and fails: could not verify,
        # and the table as it was.
        setup(drifted, FAILING_GENERATOR)
        expect(check(entry, root) == "error", "a failing generator was not refused")
        expect(table.read_bytes() == drifted, "a failing generator's damage was left")
        expect(run((entry,), root) == 1, "a failing generator did not exit 1")

        # A listed generator that is missing: refused, not skipped.
        setup(good, None)
        expect(check(entry, root) == "error", "a missing generator was skipped")
        expect(table.read_bytes() == good, "a missing generator's table was changed")

        # A listed table that is missing: refused.
        setup(None, good_generator)
        expect(check(entry, root) == "error", "a missing table was skipped")
        expect(not table.exists(), "a missing table was created")

        # Interrupted after the generator has written: the interruption goes
        # on up, and the table is restored first.
        setup(drifted, good_generator)

        def interrupted(gen: pathlib.Path, at: pathlib.Path) -> subprocess.CompletedProcess:
            table.write_bytes(b"half a table")
            raise KeyboardInterrupt

        try:
            check(entry, root, interrupted)
            expect(False, "an interruption was swallowed")
        except KeyboardInterrupt:
            expect(True, "")
        expect(table.read_bytes() == drifted, "an interrupted run left its table changed")

    print(f"{TAG} {cases} self-test case(s), {failures} failed")
    return 0 if failures == 0 else 1


def main(argv: list[str]) -> int:
    unknown = selftestflag.unknown_options(argv)
    if unknown:
        log(f"unrecognised option {unknown[0]!r}: nothing was checked")
        return 2
    if selftestflag.wants_selftest(argv):
        return self_test()
    return run(TABLES)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
