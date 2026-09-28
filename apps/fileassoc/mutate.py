"""Mutation test for applying one program to a whole group of file types.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers the test changed on 2026-09-27 for lane C
(requests/c-e-a-test-that-counts-the-toolkits-audio-types.md): it wrote the
audio group out -- ten types, "5 of 10" -- and now asks the tables the dialog
reads, so a type added to the group changes the numbers and not the verdict.
These rows show it still fails when the group code is broken.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

GROUP = "applying_to_a_group_sets_what_it_can_and_names_what_it_cannot"

MUTATIONS = [
    (
        "the group stops at the first type it cannot take",
        "                Err(e) => outcome.skipped.push((extension.to_string(), e.to_string())),",
        "                Err(_) => break,",
        [GROUP],
    ),
    (
        "what was set is not counted",
        "                Ok(()) => outcome.set.push(extension.to_string()),",
        "                Ok(()) => {}",
        [GROUP],
    ),
    (
        "the total is what was set, not the group",
        "                        let total = category.extensions().count();\n                        self.status = if group.skipped.is_empty() {",
        "                        let total = group.set.len();\n                        self.status = if group.skipped.is_empty() {",
        [GROUP],
    ),
    (
        "the types skipped are not named",
        '                                missed.join(" ")',
        "                                String::new()",
        [GROUP],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "fileassoc", timeout=900, only=only))
