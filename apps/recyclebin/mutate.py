"""Mutation test for the recycle bin's cross-drive move.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: the copy the bin falls back to across
drives followed links -- a link to a folder inside what was recycled had the
folder's contents copied into the bin.  It carries links as links now.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

NO_FOLLOW = "the_cross_drive_copy_never_follows_a_link"

MUTATIONS = [
    (
        "the copy follows links",
        "        let kind = entry.file_type()?;",
        "        let kind = fs::metadata(entry.path())?.file_type();",
        [NO_FOLLOW],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "recyclebin", timeout=900))
