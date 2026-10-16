"""Mutation test for termchild: the program on the far side of a terminal.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.

The rows cover finding a program along `PATH` (`find_program`), added on
2026-10-09 for `terminal -e`. Its execute-bit check is `#[cfg(unix)]` and its
test with it; this host builds the other arm, so that line has no row here.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

FOUND = "a_program_is_found_along_the_path_as_execvp_finds_it"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a name with a slash is searched for along the path",
        "    if program.as_encoded_bytes().contains(&b'/') {\n"
        "        return Some(PathBuf::from(program));\n"
        "    }\n",
        "",
        [FOUND],
    ),
    (
        "a directory of the program's name is taken for it",
        "    if !meta.is_file() {\n        return false;\n    }\n",
        "",
        [FOUND],
    ),
    (
        "the first directory's candidate is taken whatever it is",
        "        .find(|candidate| is_executable_file(candidate))\n",
        "        .next()\n",
        [FOUND],
    ),
]
# No row for the empty PATH entry (the current directory, by POSIX): testing it
# would put a program in the test's working directory, which tests must not
# write to.

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "termchild", timeout=300, only=only))
