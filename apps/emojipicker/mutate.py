"""Mutation test for the emoji picker.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The rows cover the search field drawn by the toolkit in the theme's shape
(`guitk::field::draw`, lane C's c-e-a-theme-can-shape-the-controls) and what
the pointer lights.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

FIELD = "the_search_field_is_the_toolkits_lit_under_the_pointer_and_focused_at_the_users_width"
LEAVE = "leaving_the_window_puts_out_the_cell_under_the_pointer"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "the field is drawn the same wherever the pointer is",
        "                hovered: self.search_hovered,",
        "                hovered: false,",
        [FIELD],
    ),
    (
        "the field is drawn the same whether it has the keyboard or not",
        "                focused: self.search_focused,",
        "                focused: false,",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();",
        "        let _ = settings;",
        [FIELD],
    ),
    (
        "the pointer over the field is not noticed",
        "            state.search_hovered = matches!(target, Some(Target::SearchField));",
        "            let _ = &target;",
        [FIELD],
    ),
    (
        "the pointer leaving the window leaves the field lit",
        "            state.hovered_emoji = Option::None;\n            state.search_hovered = false;\n",
        "            state.hovered_emoji = Option::None;\n",
        [FIELD],
    ),
    (
        "the pointer leaving the window leaves its cell lit",
        "            state.hovered_emoji = Option::None;\n            state.search_hovered = false;\n",
        "            state.search_hovered = false;\n",
        [LEAVE],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "emojipicker", timeout=600, only=only))
