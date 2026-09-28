"""Mutation test for memory's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table starts with the theme (the operator's answer to C-Q16, §1422): the
card backs, the table and the chrome follow the user's palette, and the
faces keep their eight hues, each in the shade that reads on its card.

Usage:  python -u apps/memory/mutate.py [substring ...]
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
        "a face is always written in its pale shade",
        "            .map_or(self.chrome.text, |&pair| gamechrome::legible_on(pair, card))",
        "            .map_or(self.chrome.text, |&(pale, _)| pale)",
        ["every_faces_letter_reads_on_its_card_in_either_theme"],
    ),
    (
        "every face is written in the first hue",
        "            .and_then(|i| HUES.get(i))",
        "            .and_then(|_| HUES.first())",
        ["every_faces_letter_reads_on_its_card_in_either_theme"],
    ),
    (
        "the keyboard's ring is drawn in the card's own colour",
        "                    ring(f, card, (gutter * 0.9).max(1.0), c.chrome.ring);",
        "                    ring(f, card, (gutter * 0.9).max(1.0), back);",
        ["the_cursor_ring_follows_the_card_it_is_on"],
    ),
    (
        "the help sheet's scrim is Mocha's whatever the theme",
        "    fill(f, l.window, c.chrome.scrim, 0.0);",
        "    fill(f, l.window, Color::rgba(0x1E, 0x1E, 0x2E, 158), 0.0);",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "memory", timeout=240))
