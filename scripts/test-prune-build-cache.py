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

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import srcload  # noqa: E402 - the path has to be set up before this import

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

    `srcload`, not `importlib.util.spec_from_file_location`, because the
    latter consults `__pycache__` and decides the bytecode is current from the
    source's `(mtime, size)` -- and the recorded mtime has one-second
    resolution.  Two same-size writes inside one second therefore run the
    *first* one, which is the shape every mutation-test and edit-rerun loop
    has.  This wrapper had that defect until it was noticed by the `.pyc` it
    left behind; see `srcload.py`'s own docstring for how it was originally
    found.
    """
    return srcload.load(os.path.join(HERE, "prune-build-cache.py"), "prune_build_cache")


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
