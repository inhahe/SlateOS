#!/usr/bin/env python3
"""Refuse a programs.md that is not what the workspace generates now.

WHY THIS EXISTS
---------------
The operator asked for a list of every program with a line on what each does,
and for "a rule somewhere that if you make a program, record it somewhere so
everybody knows it exists" (design-decisions §1053, the answer to B-Q21). The
list is `programs.md`, generated from the workspace by lane B's
`scripts/program-catalogue.py`: each program's description is the first
sentence of its own module doc. The rule needs a gate, or a lane that adds a
program and does not regenerate the list has broken it with nothing saying so
(requests/b-a-a-gate-for-the-program-catalogue.md).

WHY IT IS A SCRIPT OF ITS OWN
-----------------------------
The generator's bare run *writes* `programs.md` -- it is the command a lane runs
to fix the list -- and refuses only under `--check`. A gate run bare (as
`scripts/pre-boot.py` runs every gate, and as `check-gates-can-refuse.py`
requires every gate to be able to refuse) would then regenerate the tree it was
asked to judge and report success. So the gate is this file: run bare, it asks
the generator `--check` and refuses with its answer; `--self-test` runs the
generator's own cases. The generator, the list and their workflow stay lane B's.

usage: check-program-catalogue.py [--self-test]

Exit codes: 0 the list is current; 1 it is stale (the fix is printed: run
`python scripts/program-catalogue.py` and commit `programs.md`); 2 the generator
is missing or an option was not understood.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

GENERATOR = Path(__file__).resolve().parent / "program-catalogue.py"


def run_generator(flag: str) -> int:
    """The generator's exit status for `flag`, its output passed through."""
    if not GENERATOR.is_file():
        print(f"check-program-catalogue: {GENERATOR.name} is missing, so the "
              "list cannot be checked")
        return 2
    sys.stdout.flush()
    return subprocess.run([sys.executable, str(GENERATOR), flag],
                          check=False).returncode


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return run_generator("--self-test")
    unknown = selftestflag.unknown_options(argv, known=())
    if unknown:
        print("check-program-catalogue: unknown option(s): " + ", ".join(unknown))
        return 2
    rc = run_generator("--check")
    if rc == 1:
        print("check-program-catalogue: programs.md must list every program the "
              "workspace builds. Regenerate it, and commit it with the program:\n"
              "    python scripts/program-catalogue.py")
    return rc


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
