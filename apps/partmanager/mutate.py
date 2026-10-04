"""Mutation test for the partition manager.

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
        "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n"
        "                app.show_help = false;\n"
        "                return EventResult::Consumed;\n"
        "            }\n",
        "",
        [CARD],
    ),
    (
        "only the left button puts the card away",
        "            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n",
        "            MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n",
        [CARD],
    ),
    (
        "a press under the card leaves it up",
        "                app.show_help = false;\n"
        "                return EventResult::Consumed;\n",
        "                return EventResult::Consumed;\n",
        [CARD],
    ),
    (
        "the wheel scrolls what the card covers",
        "            MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n",
        "",
        [CARD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "partmanager", timeout=900, only=only))
