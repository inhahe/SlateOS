"""Mutation test for the credential manager's Copy.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-27: Copy said "Copied Password --
clears in 30s" over a clipboard only this program could read, so nothing
could be pasted anywhere else.  It now copies nothing and says why, and the
tests pin that the press is answered, that the words are drawn, and that no
"Copied" survives anywhere.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

REFUSAL = "a_copy_press_says_nothing_was_copied_and_why"
PAST_END = "a_copy_target_past_the_last_field_does_nothing"

MUTATIONS = [
    (
        "a Copy press is not answered",
        "    state.copy_refused = Some((*label).to_string());",
        "    let _ = label;",
        [REFUSAL],
    ),
    (
        "the refusal is not drawn",
        "    if let Some(label) = state.copy_refused.as_deref() {",
        "    if let Some(label) = None::<&str> {",
        [REFUSAL],
    ),
    (
        "a press past the fields is answered",
        "    let Some((label, value)) = fields.get(index) else {\n        return false;\n    };",
        "    let Some((label, value)) = fields.get(index).or(fields.first()) else {\n        return false;\n    };",
        [PAST_END],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "credmanager", timeout=600, only=only))
