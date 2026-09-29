#!/usr/bin/env python3
"""Self-test for check-mutation-needles.py: a harness whose rows all match
passes, and one row gone stale is named and fails the run.

usage: python scripts/test-check-mutation-needles.py
"""
import pathlib
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
CHECK = HERE / "check-mutation-needles.py"

SOURCE = """fn main() {
    let x = 1;
    println!("{x}");
}
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
    ("x is two", "    let x = 1;", "    let x = 2;", ["a_test"]),
]
TABLES = {"main.rs": MAIN}
if __name__ == "__main__":
    raise SystemExit("a harness was run, not read")
"""


def run(root, text):
    crate = root / "crate"
    (crate / "src").mkdir(parents=True, exist_ok=True)
    (crate / "src" / "main.rs").write_text(SOURCE, encoding="utf-8")
    (crate / "mutate.py").write_text(text, encoding="utf-8")
    return subprocess.run(
        [sys.executable, str(CHECK), str(crate)],
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
            "every row matches",
            HARNESS.format(second='("y", "    println!(\\"{x}\\");", "", ["a_test"]),'),
            0,
        ),
        case(
            "a row whose needle matches nothing is named and fails",
            HARNESS.format(second='("gone", "    let y = 3;", "", ["a_test"]),'),
            1,
            "'gone'",
        ),
        case(
            "a row whose needle matches twice is named and fails",
            HARNESS.format(second='("twice", "x", "", ["a_test"]),'),
            1,
            "'twice'",
        ),
        case(
            "a source named as a directory is read file by file",
            DIRECTORY_HARNESS,
            0,
        ),
    ]
    failed = results.count(False)
    print(f"\n{len(results)} self-test case(s), {failed} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
