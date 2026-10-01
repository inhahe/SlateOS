"""Mutation test for lights-out's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table is the theme (the operator's answer to C-Q16, §1422): everything
follows the user's palette, the lights and the cursor are seen in either
theme, the buttons are the toolkit's, and every text reads on what is under
it.

Usage:  python -u apps/lightsout/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "the theme is never taken up",
        "        self.palette = *palette;\n        self.colours = Colours::of(palette);",
        "        let _ = palette;",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "the colours are not rebuilt with the palette",
        "        self.colours = Colours::of(palette);",
        "        let _ = Colours::of(palette);",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "a lit cell is the grey of an unlit one's neighbour",
        "            light_on: p.ink(p.yellow),",
        "            light_on: p.surface1,",
        ["the_lights_and_the_cursor_are_seen_in_either_theme"],
    ),
    (
        "the cursor is the palette's blue whatever the cell",
        "            cursor_on: gamechrome::legible_on((p.text, p.base), p.ink(p.yellow)),",
        "            cursor_on: p.ink(p.blue),",
        ["the_lights_and_the_cursor_are_seen_in_either_theme"],
    ),
    (
        "the cursor takes the unlit cell's colour on a lit one",
        "        let ring = if on {\n            self.colours.cursor_on\n",
        "        let ring = if false {\n            self.colours.cursor_on\n",
        ["the_cursor_as_drawn_is_seen_on_the_cell_under_it"],
    ),
    (
        "the best line is the faintest grey",
        "            // grey is 2.3:1 on a light page.\n            self.colours.subtext0,",
        "            // grey is 2.3:1 on a light page.\n            self.palette.overlay0,",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "the board size in play looks like the others",
        "            self.button(f, r, &format!(\"{size}x{size}\"), l.small, current);",
        "            self.button(f, r, &format!(\"{size}x{size}\"), l.small, false);",
        ["the_board_size_in_play_looks_chosen"],
    ),
    (
        "the banner has no ground of its own",
        "(r.h * 0.15).min(8.0), Surface::Panel)",
        "(r.h * 0.15).min(8.0), Surface::Card)",
        ["the_banner_and_the_sheet_have_grounds_of_their_own"],
    ),
    (
        "the help sheet has no ground of its own",
        "            .push_surface(f, p.x, p.y, p.w, p.h, 10.0, Surface::Panel);",
        "            .push_surface(f, p.x, p.y, p.w, p.h, 10.0, Surface::Card);",
        ["the_banner_and_the_sheet_have_grounds_of_their_own"],
    ),
    (
        "the help's scrim is Mocha's whatever the theme",
        "Chrome::of(&self.palette).scrim",
        "Color::rgba(0x1E, 0x1E, 0x2E, 158)",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "lightsout", timeout=240))
