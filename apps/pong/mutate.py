"""Mutation test for pong's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table is the theme (the operator's answer to C-Q16, §1422): everything
follows the user's palette, the two sides are the accent and a hue apart from
it, the buttons are the toolkit's, and every text reads on what is under it.

Usage:  python -u apps/pong/mutate.py [substring ...]
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
        "the two paddles are one colour",
        "            ai: gamechrome::apart_from_accent(p),",
        "            ai: p.ink(p.accent),",
        ["the_two_sides_are_told_apart_and_seen_on_the_field"],
    ),
    (
        "the pause button is live on the menu",
        "                    disabled: !self.enabled(*action),",
        "                    disabled: false,",
        ["the_pause_button_is_switched_off_with_no_game_to_pause"],
    ),
    (
        "the sentence is the faintest grey",
        "                // faintest grey is 2.3:1 on a light page.\n                self.colours.subtext0,",
        "                // faintest grey is 2.3:1 on a light page.\n                self.palette.overlay0,",
        ["every_text_reads_on_what_is_under_it_in_either_theme"],
    ),
    (
        "the sentence is centred on its font size",
        "(l.footer.h - text::line_height(l.font, FontWeightHint::Regular)) / 2.0",
        "(l.footer.h - l.font) / 2.0",
        ["the_footer_sentence_is_centred_on_its_line"],
    ),
    (
        "the scrim is Mocha's crust whatever the theme",
        "            fill(f, l.field, Chrome::of(&self.palette).scrim, 4.0);",
        "            fill(f, l.field, Color::rgba(0x11, 0x11, 0x1B, 150), 4.0);",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "the message has no ground of its own",
        "            .push_surface(f, r.x, r.y, r.w, r.h, 6.0, Surface::Panel);",
        "            .push_surface(f, r.x, r.y, r.w, r.h, 6.0, Surface::Card);",
        ["the_message_sits_on_a_panel_of_its_own"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "pong", timeout=240))
