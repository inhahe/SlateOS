"""Mutation test for match3's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table starts with the theme (the operator's answer to C-Q16, §1422): the
board and the chrome follow the user's palette, the seven gems keep their
colours, and a new game -- which rebuilds the game from scratch -- keeps the
window's colours.

Usage:  python -u apps/match3/mutate.py [substring ...]
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
        "the game-over veil is Mocha's whatever the theme",
        "            color: c.chrome.veil,",
        "            color: Color::rgba(17, 17, 27, 200),",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "a gem's symbol is written in the dark theme's text colour",
        "            text: String::from(gem.gem_type.symbol()),\n            color: GEM_INK,",
        "            text: String::from(gem.gem_type.symbol()),\n            color: Color::from_hex(0xCDD6F4),",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "two gems share a colour",
        "            Self::Ruby => RUBY,",
        "            Self::Ruby => SAPPHIRE,",
        ["test_gem_type_color_distinct"],
    ),
    (
        "a new game drops the user's colours",
        "        self.palette = palette;",
        "        let _ = palette;",
        ["a_new_game_keeps_the_users_colours"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "match3", timeout=240))
