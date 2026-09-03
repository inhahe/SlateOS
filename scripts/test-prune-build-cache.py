#!/usr/bin/env python3
"""Run `prune-build-cache.py`'s self-test under the boot test's tooling gate.

`scripts/boot-test.sh` globs `scripts/test-*.py` and runs every match, which is
the only thing that makes a tooling suite a *test* rather than a comment that
took an hour to write.  `prune-build-cache.py` carries its assertions inside
itself -- they need its internals, not its command line -- so this file exists
to put them in front of that glob rather than to duplicate them.

The floor below matters more here than for most suites.  This tool's job is to
*delete* things, and both of its failure modes are silent: pruning too little
looks exactly like a hot cache with nothing to prune, and pruning too much
looks exactly like a slow build nobody attributes to a script that ran days
ago.  A self-test that quietly stopped asserting would restore both.
"""

from __future__ import annotations

import importlib.util
import os
import sys

# The number of checks the suite had when this wrapper was written.  Compared
# with `<` rather than `==` so adding an assertion does not fail the build,
# while losing most of them -- a refactor that drops a block, an exception
# swallowed mid-suite -- does.
MIN_CHECKS = 15


def load():
    """Import the hyphenated script as a module.

    `prune-build-cache` is not a legal identifier, so it cannot be imported by
    name; the file is loaded by path instead.  It is named for the command it
    provides, and renaming it to suit Python's import rules would make the
    command harder to find for the sake of this one caller.
    """
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "prune-build-cache.py")
    spec = importlib.util.spec_from_file_location("prune_build_cache", path)
    if spec is None or spec.loader is None:
        print(f"FATAL: cannot load {path}")
        sys.exit(1)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def main():
    mod = load()
    checks, failures = mod.self_test()

    if checks < MIN_CHECKS:
        print(
            f"FATAL: the self-test ran only {checks} check(s); it has at least "
            f"{MIN_CHECKS}. Discovery is broken, not the code."
        )
        return 1
    if failures:
        print(f"{failures} FAILED")
        return 1
    print(f"all {checks} prune-build-cache tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
