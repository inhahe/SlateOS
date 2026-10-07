"""Mutation test for the unit converter's controls and its From box.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

Covers what changed on 2026-10-04.  The press carried its own copy of the
main area's geometry, and it had drifted from the drawing -- the From box
answered twelve points below its box, the unit buttons, the unit lists and
the favourites sixteen, the favourites toggle fourteen, the star forty and
the swap button a hundred and fifteen -- so both now read one `Layout`, and
`every_control_answers_where_it_is_drawn` aims each press at the control's
ink as drawn.  The From box is the toolkit's field (lane C,
c-e-a-theme-can-shape-the-controls), and its caret stands where the text
before it ends rather than a fixed 8.4 points a character.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

DRAWN = "every_control_answers_where_it_is_drawn"
FIELD = "the_from_box_is_the_toolkits_field"
CARET = "the_caret_stands_where_the_text_before_it_ends"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- Every control where it is drawn --
    (
        "the swap button answers nothing",
        "        if layout.on_swap(x, y) {\n",
        "        if false && layout.on_swap(x, y) {\n",
        [DRAWN],
    ),
    (
        "the star answers nothing",
        "        if layout.star.contains(x, y) {\n",
        "        if false && layout.star.contains(x, y) {\n",
        [DRAWN],
    ),
    (
        "a unit list's rows answer twelve points low",
        "        let into = y - list.y - LIST_PAD;\n",
        "        let into = y - list.y - LIST_PAD - 12.0;\n",
        [DRAWN],
    ),
    (
        "the favourites toggle answers nothing",
        "        if layout.favourites_toggle.contains(x, y) {\n",
        "        if false && layout.favourites_toggle.contains(x, y) {\n",
        [DRAWN],
    ),
    (
        "a favourite answers nothing",
        "            && let Some(idx) = layout.favourite_at(self.favorites.len(), x, y)\n",
        "            && let Some(idx) = layout.favourite_at(0, x, y)\n",
        [DRAWN],
    ),
    (
        "the From box answers twelve points below itself",
        "        if layout.from_box.contains(x, y) {\n",
        "        if layout.from_box.translated(0.0, 12.0).contains(x, y) {\n",
        [DRAWN],
    ),
    # -- The From box is the toolkit's field --
    (
        "the From box never lights",
        "            hovered: open\n",
        "            hovered: false\n",
        [FIELD],
    ),
    (
        "the From box shows past an open unit list",
        "        let open = !self.list_open();\n",
        "        let open = true;\n",
        [FIELD],
    ),
    (
        "the From box never says its value is no number",
        "            invalid: !self.from_input.is_empty() && self.from_input.parse::<f64>().is_err(),\n",
        "            invalid: false,\n",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
    (
        "a change in the light asks for no redraw",
        "        if self.input_box_state().hovered == lit {\n",
        "        if true {\n",
        [FIELD],
    ),
    # -- The caret --
    (
        "the caret is placed a fixed width a character",
        "guitk::text::measure(before, INPUT_FONT, FontWeightHint::Regular))",
        "before.len() as f32 * 8.4)",
        [CARET],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "unitconverter", timeout=600, only=only))
