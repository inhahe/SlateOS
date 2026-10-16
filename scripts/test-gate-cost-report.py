#!/usr/bin/env python3
"""Run `gate-cost-report.py`'s self-test under the boot test's tooling gate.

`scripts/boot-test.sh` globs `scripts/test-*.py` and runs every match. The
report carries its assertions inside itself, because they need its internals
rather than its command line, so this file exists to put them in front of that
glob, not to duplicate them. Nothing else runs them: the report is not a gate,
so `check-gates-are-wired.py` does not ask after its self-test.

Its failure mode is the one that makes a measurement worse than none. If a
fixture's refusals were counted as a gate's catches, or a gate's rows dropped
as fixture noise, the report would still print a confident table, and the
table would then decide which gates are cached, narrowed or retired
(design-decisions 974).
"""

from __future__ import annotations

import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

#: The number of `ok` lines the self-test printed when this wrapper was
#: written. `<` rather than `==`, so adding a check does not fail the build,
#: while losing most of them does.
MIN_CHECKS = 11


def main() -> int:
    proc = subprocess.run(
        [sys.executable, os.path.join(HERE, "gate-cost-report.py"), "--self-test"],
        capture_output=True, text=True,
    )
    sys.stdout.write(proc.stdout)
    sys.stderr.write(proc.stderr)
    oks = sum(1 for ln in proc.stdout.splitlines() if ln.lstrip().startswith("ok "))
    if proc.returncode != 0:
        print(f"FAIL  gate-cost-report.py --self-test exited {proc.returncode}")
        return 1
    if oks < MIN_CHECKS:
        print(f"FAIL  only {oks} self-test check(s) ran, fewer than {MIN_CHECKS}: "
              "the suite has lost assertions")
        return 1
    print(f"all gate-cost-report tests passed ({oks} checks)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
