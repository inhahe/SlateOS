#!/usr/bin/env python3
"""Self-test for verify_mutations.py: a harness whose rows all hold passes;
a dead or ambiguous anchor, a test that is not there, or a duplicate row
name is named and fails the run; a source named as a directory is read file
by file, and a test in the crate's `tests/` counts.

usage: python scripts/test-verify_mutations.py
"""
import pathlib
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
VERIFY = HERE / "verify_mutations.py"

SOURCE = """fn main() {
    let x = 1;
    println!("{x}");
}

#[test]
fn a_test() {}
"""

HARNESS = """from pathlib import Path
SRC = Path(__file__).parent / "src" / "main.rs"
MUTATIONS = [
    ("x is two", "    let x = 1;", "    let x = 2;", ["a_test"]),
    {second}
]
if __name__ == "__main__":
    raise SystemExit("a harness was run, not read")
"""

DIRECTORY_HARNESS = """from pathlib import Path
SRC = Path(__file__).parent / "src"
MAIN = [
    ("x is two", "    let x = 1;", "    let x = 2;", ["a_test", "an_integration_test"]),
]
TABLES = {"main.rs": MAIN}
if __name__ == "__main__":
    raise SystemExit("a harness was run, not read")
"""


def run(root, text):
    crate = root / "crate"
    (crate / "src").mkdir(parents=True, exist_ok=True)
    (crate / "tests").mkdir(parents=True, exist_ok=True)
    (crate / "src" / "main.rs").write_text(SOURCE, encoding="utf-8")
    (crate / "tests" / "outside.rs").write_text(
        "#[test]\nfn an_integration_test() {}\n", encoding="utf-8"
    )
    (crate / "mutate.py").write_text(text, encoding="utf-8")
    return subprocess.run(
        [sys.executable, str(VERIFY), str(crate)],
        capture_output=True,
        text=True,
        check=False,
    )


def case(name, text, want_code, want_in_output=None):
    with tempfile.TemporaryDirectory() as tmp:
        got = run(pathlib.Path(tmp), text)
    ok = got.returncode == want_code and (
        want_in_output is None or want_in_output in got.stdout
    )
    print(f"{'ok  ' if ok else 'FAIL'} {name}")
    if not ok:
        print(f"     exit {got.returncode}, wanted {want_code}")
        print("     " + got.stdout.replace("\n", "\n     "))
        print("     " + got.stderr.replace("\n", "\n     "))
    return ok


def main():
    results = [
        case(
            "every row holds",
            HARNESS.format(second='("y", "    println!(\\"{x}\\");", "", ["a_test"]),'),
            0,
        ),
        case(
            "an anchor that matches nothing is named and fails",
            HARNESS.format(second='("gone", "    let y = 3;", "", ["a_test"]),'),
            1,
            "ANCHOR main.rs x0: gone",
        ),
        case(
            "an anchor that matches twice is named and fails",
            HARNESS.format(second='("twice", "x", "", ["a_test"]),'),
            1,
            ": twice",
        ),
        case(
            "a test that is not there is named and fails",
            HARNESS.format(second='("y", "    println!(\\"{x}\\");", "", ["no_such"]),'),
            1,
            "NO SUCH TEST no_such",
        ),
        case(
            "a row name used twice in a table is named and fails",
            HARNESS.format(second='("x is two", "    println!(\\"{x}\\");", "", ["a_test"]),'),
            1,
            "DUPLICATE ROW NAME: x is two",
        ),
        case(
            "a directory source is read file by file, and tests/ counts",
            DIRECTORY_HARNESS,
            0,
        ),
        case(
            "a harness that will not import is unreadable",
            "this is not python\n",
            2,
            "cannot be read",
        ),
    ]
    failed = results.count(False)
    print(f"\n{len(results)} self-test case(s), {failed} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
