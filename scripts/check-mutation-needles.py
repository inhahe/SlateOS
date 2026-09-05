#!/usr/bin/env python3
"""Fail if a gate's mutation table has rotted into a table of dead needles.

The rule
--------
**Every row of every `SELFTEST_MUTANTS` table must still quote a line that
exists, exactly once, in the gate that carries it.**

Why this needs a gate of its own
--------------------------------
`scripts/mutate-gate.py` breaks one piece of a gate at a time and demands its
self-test notice. It is the only thing in this tree that can tell a guard that
works from a guard that is decoration -- twice now the answer has been
"decoration", and both times a fully green suite was the symptom rather than
the diagnosis.

But the sweep **rewrites the gate on disk** and restores it in a `finally`.
That is fine for a tool a person runs deliberately; it is not something that
can be wired into a boot test in a tree three lanes share and one QEMU lock
serialises. Two sweeps at once is not hypothetical -- it happened while
`mutate-gate.py` was being written, and the second sweep's "original" was the
first sweep's mutant. So the sweep stays manual.

Manual means occasional, and occasional is exactly what a mutation needle
cannot survive. A needle is a *quotation* of the gate's source. Reword the
line it quotes -- a rename, a reflow, an added argument -- and the needle
matches nothing. It does not fail: it is skipped, and it leaves behind a row
of a table that reads as coverage. The gate it was supposed to protect goes
back to being untested, and the only thing that would say so is the sweep
nobody has run since.

The half of the sweep that finds this needs no mutation at all: count the
occurrences. That half is static, and it is what this gate runs on every boot
test. It cannot tell you a mutant *survived* -- only the sweep can -- but it
can tell you that the question stopped being asked, which is the failure that
happens silently.

Why not simply wire the whole sweep, then
-----------------------------------------
Once mutants are written beside the gate rather than over it, running the real
sweep unattended is safe, so the question is only what it costs. Measured
2026-09-03, on a host busy with a boot test: sweeping the three tables took
**9m38s**, against 24s for this gate on the same host. The sweep is one
subprocess per needle and each one runs a gate's entire suite, so that figure
grows with every row anyone adds -- the cost of the gate that would catch a
survivor rises in proportion to how much anyone tests.

Nine minutes is not affordable per boot on the same run that already spends
forty minutes on gates, and a gate people are tempted to skip is worse than no
gate. So the sweep stays a thing you run deliberately, and what runs every
time is the check that the sweep is still *able* to say anything.

What is checked, for every `scripts/*.py`
-----------------------------------------
1. **`SELFTEST_MUTANTS` is a literal.** Read with `ast.literal_eval`, never by
   executing the gate. A table assembled at runtime is one this gate and the
   sweep could read differently, and two readings of the same table is the
   failure the table exists to prevent.
2. **Every needle occurs exactly once outside the table itself.** Zero is a
   stale quotation. Two or more is a needle that cannot say which site it
   meant, and the sweep refuses to guess.
3. **No replacement is identical to its needle**, which would mutate nothing
   and be indistinguishable from a mutant every case killed.
4. **A gate that declares a table has a self-test to run it.** A table on a
   gate the sweep cannot invoke is decoration of exactly the kind the table
   was added to detect.

Usage:
    python scripts/check-mutation-needles.py [--root scripts] [--self-test]

Exit codes:
    0  every needle in every table is live
    1  at least one is not (they are listed)
    2  the scan inspected too little to mean anything, or could not run at
       all -- never confused with "nothing is wrong", because a check that
       cannot fire must not be indistinguishable from a check that passes.
"""

import argparse
import contextlib
import importlib.util
import io
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent


def default_root() -> Path:
    """The directory the gates live in, which is this script's own."""
    return HERE


def harness():
    """`mutate-gate.py` as a module, despite the hyphen that bars `import`.

    The table-reading and needle-counting live there, not here, on purpose.
    The sweep applies the same two rules before it mutates anything, and a
    gate that agreed with the sweep only by having been written from the same
    description would be free to drift out of agreement later -- which is the
    same defect as a stale needle, one level up.
    """
    path = HERE / "mutate-gate.py"
    spec = importlib.util.spec_from_file_location("mutate_gate", path)
    if spec is None or spec.loader is None:
        raise FileNotFoundError(str(path))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


#: Floors for a whole-directory run. Not targets: they fire on a scan that
#: COLLAPSED and never on one that merely shrank. Set 2026-09-03, when three
#: tables carried thirty-eight needles between them and the smallest alone
#: held six. A scan that finds less than one table's worth has
#: stopped looking, and the honest answer then is a refusal rather than the
#: "all live" a scan of nothing would otherwise print.
MIN_TABLES = 2
MIN_NEEDLES = 6

SELFTEST_MUTANTS = [
    ("MIN_TABLES gutted to 0", "MIN_TABLES = 2", "MIN_TABLES = 0"),
    ("MIN_NEEDLES gutted to 0", "MIN_NEEDLES = 6", "MIN_NEEDLES = 0"),
    ("the floor is never consulted at all",
     "    if tables < MIN_TABLES or needles < MIN_NEEDLES:",
     "    if False:"),
    ("only the needle half of the floor is checked",
     "    if tables < MIN_TABLES or needles < MIN_NEEDLES:",
     "    if needles < MIN_NEEDLES:"),
    ("only the table half of the floor is checked",
     "    if tables < MIN_TABLES or needles < MIN_NEEDLES:",
     "    if tables < MIN_TABLES:"),
    ("tables are never counted", "        tables += 1", "        pass"),
    ("needles are never counted",
     "        needles += len(table)", "        pass"),
    ("findings are collected and then ignored",
     "    if findings:", "    if False:"),
    ("a dead needle is not a finding",
     "        for _row, label, reason in mg.needle_problems(source, table):",
     "        for _row, label, reason in []:"),
    ("a table no self-test can sweep is not a finding",
     "        if not any(flag in source for flag in mg.SELFTEST_FLAGS):",
     "        if False:"),
    ("a table that is not a literal is not a finding",
     '            findings.append(f"{path.name}: SELFTEST_MUTANTS {exc}")',
     "            pass"),
    ("a mutant a sweep is running right now is judged as a gate",
     '        if path.name.startswith("."):', "        if False:"),
    ("--root is accepted and then ignored",
     "    root = Path(args.root) if args.root else default_root()",
     "    root = default_root()"),
    ("a collapsed scan is reported as a finding instead of a refusal",
     '              f"stopped looking rather than that everything is well.")\n'
     "        return 2",
     '              f"stopped looking rather than that everything is well.")\n'
     "        return 1"),
    ("a dead needle is reported as a refusal instead of a finding",
     '        print("\\nEach is a row that no longer asks anything. Re-quote it "\n'
     '              "against the line as it now reads, or delete it.")\n'
     "        return 1",
     '        print("\\nEach is a row that no longer asks anything. Re-quote it "\n'
     '              "against the line as it now reads, or delete it.")\n'
     "        return 2"),
]


def scan(root: Path, mg) -> tuple[list[str], int, int]:
    """Findings, tables seen, needles seen, over every `*.py` under `root`."""
    findings: list[str] = []
    tables = needles = 0
    for path in sorted(root.glob("*.py")):
        # A sweep in flight writes each mutant to `.mutant-<pid>-<gate>` beside
        # the gate. Judging one of those is judging a file that is deliberately
        # broken and will not exist a second later, so the verdict would be
        # both wrong and irreproducible.
        if path.name.startswith("."):
            continue
        try:
            source = path.read_text(encoding="utf-8")
        except OSError as exc:
            findings.append(f"{path.name}: unreadable ({exc})")
            continue
        try:
            table = mg.table_from_source(source)
        except SyntaxError as exc:
            findings.append(f"{path.name}: does not parse ({exc})")
            continue
        except mg.BadTable as exc:
            findings.append(f"{path.name}: SELFTEST_MUTANTS {exc}")
            continue
        if table is None:
            continue
        tables += 1
        needles += len(table)
        if not any(flag in source for flag in mg.SELFTEST_FLAGS):
            findings.append(
                f"{path.name}: declares a table but names neither "
                f"{' nor '.join(mg.SELFTEST_FLAGS)}, so no sweep can run it")
        for _row, label, reason in mg.needle_problems(source, table):
            findings.append(f"{path.name}: {label}: {reason}")
    return findings, tables, needles


def report(root: Path, mg) -> int:
    findings, tables, needles = scan(root, mg)

    # The floor before the findings, because a collapsed scan's findings are
    # about whatever fraction it happened to reach. "I could not look" is the
    # more honest headline than "here is what I found in the part I saw".
    if tables < MIN_TABLES or needles < MIN_NEEDLES:
        print(f"REFUSING A VERDICT: {tables} mutation table(s) and {needles} "
              f"needle(s) in {root} is below the floor of {MIN_TABLES} and "
              f"{MIN_NEEDLES}.\n"
              f"       A scan this small is far likelier to mean the scan "
              f"stopped looking rather than that everything is well.")
        return 2

    if findings:
        print(f"DEAD MUTATION NEEDLES -- {len(findings)} problem(s):\n")
        for f in findings:
            print(f"  {f}")
        print("\nEach is a row that no longer asks anything. Re-quote it "
              "against the line as it now reads, or delete it.")
        return 1

    print(f"ok -- {tables} mutation table(s), {needles} needle(s), all live.")
    return 0


def self_test() -> int:
    """Drive the whole gate over synthetic gate directories.

    Synthetic and not the real `scripts/` for one reason: `mutate-gate.py`
    sweeps this file by rewriting it and re-running this function, so any case
    that consulted the real tree would be judging a mutant against a directory
    that contains the mutant. The real tree is covered anyway -- by the gate's
    own `run_checker` call in the boot test, which is the run that matters.
    """
    checks = bad = 0

    def check_(label, ok):
        nonlocal checks, bad
        checks += 1
        if ok:
            print(f"ok   {label}")
        else:
            print(f"selftest FAIL: {label}", file=sys.stderr)
            bad += 1

    mg = harness()

    def gate(rows, code_lines, flag="--self-test"):
        """Source for a synthetic gate: a docstring, a table, then code."""
        table = "SELFTEST_MUTANTS = [\n"
        for row in rows:
            table += f"    {row!r},\n"
        table += "]\n"
        return (f'"""A synthetic gate naming {flag}."""\n'
                + table + "".join(line + "\n" for line in code_lines))

    def run(argv):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            rc = main(argv)
        return rc, buf.getvalue()

    # --- reading the table -------------------------------------------------
    check_("a file with no table is not a table",
           mg.table_from_source("x = 1\n") is None)
    good = gate([("l", "LIMIT = 9", "LIMIT = 0")], ["LIMIT = 9"])
    check_("a literal table is read back verbatim",
           mg.table_from_source(good) == [("l", "LIMIT = 9", "LIMIT = 0")])
    for source, why in (
        ("SELFTEST_MUTANTS = build()\n", "a table built by a call"),
        ("SELFTEST_MUTANTS = {}\n", "a table that is not a list"),
        ("SELFTEST_MUTANTS = [('a', 'b')]\n", "a row that is not a triple"),
        ("SELFTEST_MUTANTS = [('a', 'b', 3)]\n", "a row holding a non-string"),
    ):
        raised = False
        try:
            mg.table_from_source(source)
        except mg.BadTable:
            raised = True
        check_(f"{why} is refused, not read", raised)

    # --- counting needles --------------------------------------------------
    live = gate([("l", "LIMIT = 9", "LIMIT = 0")], ["LIMIT = 9"])
    check_("a needle quoting a line that exists once is live",
           mg.needle_problems(live, mg.table_from_source(live)) == [])
    stale = gate([("l", "LIMIT = 9", "LIMIT = 0")], ["LIMIT = 10"])
    check_("a needle quoting a line that no longer exists is dead",
           "matches 0 time(s)" in mg.needle_problems(
               stale, mg.table_from_source(stale))[0][2])
    twice = gate([("l", "LIMIT = 9", "LIMIT = 0")], ["LIMIT = 9", "LIMIT = 9"])
    check_("a needle quoting two sites cannot say which it meant",
           "matches 2 time(s)" in mg.needle_problems(
               twice, mg.table_from_source(twice))[0][2])
    noop = gate([("l", "LIMIT = 9", "LIMIT = 9")], ["LIMIT = 9"])
    check_("a replacement identical to its needle mutates nothing",
           "identical" in mg.needle_problems(
               noop, mg.table_from_source(noop))[0][2])

    # --- end to end, over whole synthetic directories ----------------------
    three = [("a", "A = 1", "A = 0"), ("b", "B = 2", "B = 0"),
             ("c", "C = 3", "C = 0")]
    body = ["A = 1", "B = 2", "C = 3"]
    eight = [(f"n{i}", f"N{i} = {i}", f"N{i} = 0") for i in range(8)]
    eight_body = [f"N{i} = {i}" for i in range(8)]

    def directory(files):
        td = tempfile.mkdtemp(prefix="needles-")
        for name, text in files.items():
            (Path(td) / name).write_text(text, encoding="utf-8", newline="")
        return td

    for files, want, must_say, why in (
        ({"one.py": gate(three, body), "two.py": gate(three, body)},
         0, "all live",
         "two clean tables of three needles each clear the floor and pass"),
        ({"one.py": gate(three, body),
          "two.py": gate(three, ["A = 1", "B = 2", "D = 3"])},
         1, "matches 0 time(s)",
         "one needle quoting a reworded line fails the whole directory"),
        ({"one.py": gate(three, body),
          "two.py": gate(three, ["A = 1", "B = 2", "C = 3", "C = 3"])},
         1, "matches 2 time(s)",
         "...and so does one that now quotes two sites"),
        ({"one.py": gate(three, body),
          "two.py": gate([("a", "A = 1", "A = 1"), ("b", "B = 2", "B = 0"),
                          ("c", "C = 3", "C = 0")], body)},
         1, "identical",
         "...and so does a replacement that changes nothing"),
        ({"one.py": gate(three, body),
          "two.py": gate(three, body, flag="nothing")},
         1, "no sweep can run it",
         "a table on a gate with no self-test is decoration itself"),
        ({"one.py": gate(three, body), "two.py": gate(three, body),
          "three.py": "SELFTEST_MUTANTS = build()\n"},
         1, "not a literal",
         "a table that must be executed to be read is refused"),
        ({"one.py": gate(three, body), "two.py": gate(three, body),
          ".mutant-1-two.py": gate(three, ["A = 1", "B = 2", "D = 3"])},
         0, "all live",
         "a mutant a sweep is writing right now is not a gate to judge"),
        ({"one.py": gate(eight, eight_body)},
         2, "below the floor",
         "one table is a collapsed scan however many needles it carries"),
        ({"one.py": gate(three[:2], body[:2]),
          "two.py": gate(three[:2], body[:2])},
         2, "below the floor",
         "...and so are four needles however many tables carry them"),
        ({"one.py": "x = 1\n", "two.py": "y = 2\n"},
         2, "below the floor",
         "a directory of gates that declare nothing is refused, not passed"),
    ):
        td = directory(files)
        rc, text = run(["--root", td])
        check_(f"{why}: exit {rc} (want {want})",
               rc == want and must_say in text)

    rc, text = run(["--root", str(Path(tempfile.gettempdir()) / "no-such-dir")])
    check_("a root that does not exist is refused, not reported as clean",
           rc == 2 and "no such directory" in text)

    check_("the default root is the directory this gate lives in",
           (default_root() / "mutate-gate.py").is_file())

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--root", default=None,
                        help="directory of gates to scan (default: scripts/)")
    parser.add_argument("--self-test", action="store_true",
                        help="run this gate's own cases and exit")
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    root = Path(args.root) if args.root else default_root()
    if not root.is_dir():
        print(f"no such directory: {root}", file=sys.stderr)
        return 2
    try:
        mg = harness()
    except OSError as exc:
        print(f"cannot load the mutation harness: {exc}", file=sys.stderr)
        return 2
    return report(root, mg)


if __name__ == "__main__":
    sys.exit(main())
