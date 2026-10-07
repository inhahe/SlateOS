"""Mutation test for the font manager.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The rows cover the shortcut card's hold on the pointer: a press with the
card up puts it away and reaches nothing under it (known-issues
E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a press goes through the shortcut card",
        "        if self.show_help {\n"
        "            // The card is modal for the pointer as it is for the keys: a\n",
        "        if false {\n"
        "            // The card is modal for the pointer as it is for the keys: a\n",
        [CARD],
    ),
    (
        "only the left button puts the card away",
        "            if matches!(mouse.kind, MouseEventKind::Press(_)) {\n"
        "                self.show_help = false;\n",
        "            if matches!(mouse.kind, MouseEventKind::Press(MouseButton::Left)) {\n"
        "                self.show_help = false;\n",
        [CARD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "fontmanager", timeout=600, only=only))
