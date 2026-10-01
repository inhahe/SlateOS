"""Mutation test for the file comparer's folder comparison.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: the folder view was a model and a painter
that nothing could reach -- no key entered it, and its comparison was over
names and texts held in memory.  Ctrl+D compares two real folders now, byte
for byte within a budget, and the list opens a pair into the file view.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

FOLDERS = "two_real_folders_are_compared_file_by_file"
BYTES = "same_size_different_bytes_is_different_and_the_budget_is_honest"
PAIR = "enter_opens_the_pair_and_escape_goes_back_to_the_folders"
CTRL_D = "ctrl_d_asks_for_the_left_folder_then_the_right"

MUTATIONS = [
    (
        "Ctrl+D compares nothing",
        "            Key::D if key.modifiers.ctrl => {",
        "            Key::D if key.modifiers.ctrl && false => {",
        [CTRL_D],
    ),
    (
        "different bytes are called the same",
        "        if ba.get(..n) != Some(&*chunk_b) {",
        "        if false {",
        [BYTES],
    ),
    (
        "sizes are not compared first",
        "    if ma.len() != mb.len() {\n        return Ok(Some(false));\n    }",
        "",
        [BYTES],
    ),
    (
        "the budget is ignored",
        "    if ma.len() > *budget {\n        return Ok(None);\n    }",
        "",
        [BYTES],
    ),
    (
        "a one-sided folder lists everything in it",
        "        if one_sided.iter().any(|dir| rel.starts_with(dir)) {\n            continue;\n        }",
        "",
        [FOLDERS],
    ),
    (
        "Enter opens nothing",
        "            Key::Enter => Some(self.open_folder_entry()),",
        "            Key::Enter => Some(EventResult::Consumed),",
        [PAIR],
    ),
    (
        "Escape does not go back to the folders",
        "        if key.key == Key::Escape && self.from_folders && self.dir_compare.is_some() {",
        "        if key.key == Key::Escape && self.from_folders && self.dir_compare.is_some() && false {",
        [PAIR],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "filediff", timeout=900, only=only))
