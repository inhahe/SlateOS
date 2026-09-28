"""Mutation test for towers's suite.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table starts with the theme (the operator's answer to C-Q16, §1422): the
pegs, the base and the chrome follow the user's palette, the disks keep their
colours by size, and the buttons are the toolkit's.

Usage:  python -u apps/towers/mutate.py [substring ...]
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
        "the help sheet's scrim is Mocha's whatever the theme",
        "    fill(f, l.window, c.chrome.scrim, 0.0);",
        "    fill(f, l.window, Color::rgba(0x1E, 0x1E, 0x2E, 158), 0.0);",
        ["the_window_is_drawn_in_the_users_colours"],
    ),
    (
        "a button that would do nothing is drawn live",
        "            disabled: !live,",
        "            disabled: false,",
        ["a_button_that_would_do_nothing_is_switched_off"],
    ),
    (
        "a disk's number is the pale text colour it was",
        "const DISK_INK: Color = Color::from_hex(0x161616);",
        "const DISK_INK: Color = Color::from_hex(0xCDD6F4);",
        ["every_disks_number_reads_on_it"],
    ),
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "towers", timeout=240))
