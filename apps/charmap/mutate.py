"""Mutation test for the character map.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The rows cover the shortcut card's hold on the pointer: a press with the
card up puts it away and reaches nothing under it, and the wheel scrolls
nothing it covers (known-issues
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
        "            MouseEventKind::Press(_) if app.show_help => {\n"
        "                app.show_help = false;\n"
        "                EventResult::Consumed\n"
        "            }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the card away",
        "            MouseEventKind::Press(_) if app.show_help => {\n",
        "            MouseEventKind::Press(MouseButton::Left) if app.show_help => {\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the card covers",
        "            MouseEventKind::Scroll { .. } if app.show_help => EventResult::Ignored,\n",
        "",
        [CARD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "charmap", timeout=600, only=only))
