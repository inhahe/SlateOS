#!/usr/bin/env python3
"""Regression tests for `scripts/integration-tree-sync.py`: its self-test.

Run: `python scripts/test-integration-tree-sync.py` (exit 0 = pass, 1 = fail).

The self-test builds throwaway repositories and drives one pass after another
through them: a tree fast-forwarded, a block shown once and again after six
hours, a notification that could not be shown retried, a merge left in the
tree and a crashing pass each told after an hour. This file exists so that
the push hook (which runs `scripts/test-<name>.py` for a script a push
changes) and the boot test's sweep of `scripts/test-*.py` run it: the script
reaches into `check-lane-signals.py`, and a change there that breaks it would
otherwise surface only as a scheduled task that stopped working.
"""

import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main() -> int:
    r = subprocess.run([sys.executable, str(HERE / "integration-tree-sync.py"), "--self-test"],
                       capture_output=True, text=True, encoding="utf-8", errors="replace")
    sys.stdout.write(r.stdout)
    sys.stderr.write(r.stderr)
    if r.returncode != 0 or "self-test: ok" not in r.stdout:
        print("test-integration-tree-sync: FAILED")
        return 1
    print("test-integration-tree-sync: passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
