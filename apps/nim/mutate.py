"""Mutation test for nim's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table is the theme (the operator's answer to C-Q16, §1422): everything
follows the user's palette, the heaps are palette hues told apart, the buttons
are the toolkit's, and every text reads on what is under it.

Usage:  python -u apps/nim/mutate.py [substring ...]
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
        "two neighbouring heaps share a colour",
        "                p.ink(p.red),\n                p.ink(p.yellow),",
        "                p.ink(p.red),\n                p.ink(p.red),",
        ["neighbouring_heaps_are_told_apart_in_either_theme"],
    ),
    (
        "a heap's count is the faintest grey",
        "                        self.colours.subtext0\n",
        "                        self.palette.overlay0\n",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "Take is live when a take would be refused",
        "            self.button(f, btn, \"Take\", l.small, false, self.enabled(Action::Take));",
        "            self.button(f, btn, \"Take\", l.small, false, true);",
        ["take_is_switched_off_when_a_take_would_be_refused"],
    ),
    (
        "the board in play looks like the others",
        "            self.button(f, r, &Nim::preset_label(i), l.small, current, true);",
        "            self.button(f, r, &Nim::preset_label(i), l.small, false, true);",
        ["the_board_in_play_looks_chosen"],
    ),
    (
        "the help sheet has no ground of its own",
        "            .push_surface(f, p.x, p.y, p.w, p.h, 10.0, Surface::Panel);",
        "            .push_surface(f, p.x, p.y, p.w, p.h, 10.0, Surface::Card);",
        ["the_sheet_and_the_banner_have_grounds_of_their_own"],
    ),
    (
        "the banner has no ground of its own",
        "            .push_surface(f, r.x, r.y, r.w, r.h, (r.h * 0.2).min(10.0), Surface::Panel);",
        "            .push_surface(f, r.x, r.y, r.w, r.h, (r.h * 0.2).min(10.0), Surface::Card);",
        ["the_sheet_and_the_banner_have_grounds_of_their_own"],
    ),
    (
        "the help's scrim is Mocha's whatever the theme",
        "Chrome::of(&self.palette).scrim",
        "Color::rgba(0x1E, 0x1E, 0x2E, 158)",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "nim", timeout=240))
