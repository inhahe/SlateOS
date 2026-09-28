"""Mutation test for tetris's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table starts with the theme (the operator's answer to C-Q16, §1422): the
well and everything round it follow the user's palette, the seven pieces keep
their colours, and a restart -- which rebuilds the game from scratch -- keeps
the window's colours and size.

Usage:  python -u apps/tetris/mutate.py [substring ...]
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # ── The theme ─────────────────────────────────────────────────────────
    (
        "the theme is never taken up",
        "    fn theme_changed(&mut self, palette: &Palette) {\n        self.palette = *palette;",
        "    fn theme_changed(&mut self, palette: &Palette) {\n        let _ = palette;",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "the message box's scrim is Mocha's whatever the theme",
        "        fill(f, l.window, c.chrome.scrim, 0.0);",
        "        fill(f, l.window, Color::rgba(17, 17, 27, 180), 0.0);",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "the well's grid is Mocha's whatever the theme",
        "            grid: with_alpha(p.surface1, 70),",
        "            grid: Color::rgba(49, 50, 68, 60),",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "two pieces share a colour",
        "            Self::I => SKY,",
        "            Self::I => BLUE,",
        ["test_piece_colors_are_distinct"],
    ),
    # ── A restart is a new game, not a new window ─────────────────────────
    (
        "a restart drops the user's colours",
        "                    self.palette = palette;",
        "                    let _ = palette;",
        ["a_restart_keeps_the_users_colours_and_the_window_size"],
    ),
    (
        "a restart forgets the window's size",
        "                    (self.width, self.height) = size;",
        "                    let _ = size;",
        ["a_restart_keeps_the_users_colours_and_the_window_size"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "tetris", timeout=240))
