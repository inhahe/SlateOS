"""Mutation test for the colour picker.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The rows cover the shortcut card's hold on the pointer.

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
        "            self.show_help = false;\n"
        "            return Action::Redraw;\n"
        "        }\n"
        "        if button != MouseButton::Left {\n",
        "        if button != MouseButton::Left {\n",
        [CARD],
    ),
    (
        "only the left button puts the card away",
        "        if self.show_help {\n"
        "            self.show_help = false;\n"
        "            return Action::Redraw;\n"
        "        }\n"
        "        if button != MouseButton::Left {\n"
        "            return Action::None;\n"
        "        }\n",
        "        if button != MouseButton::Left {\n"
        "            return Action::None;\n"
        "        }\n"
        "        if self.show_help {\n"
        "            self.show_help = false;\n"
        "            return Action::Redraw;\n"
        "        }\n",
        [CARD],
    ),
    (
        "an armed eyedropper samples through the card",
        "        if self.show_help {\n"
        "            self.show_help = false;\n"
        "            return Action::Redraw;\n"
        "        }\n"
        "        if button != MouseButton::Left {\n"
        "            return Action::None;\n"
        "        }\n"
        "        // An armed eyedropper takes the click before any control does, which\n"
        "        // is the whole point of a mode: it samples what is under the pointer.\n"
        "        if self.eyedropper.active {\n"
        "            let color = self.color_under(x, y, size);\n"
        "            self.eyedrop_pick(x, y, color);\n"
        "            self.status = format!(\"Sampled {}\", color.to_hex6());\n"
        "            return Action::Redraw;\n"
        "        }\n",
        "        if button != MouseButton::Left {\n"
        "            return Action::None;\n"
        "        }\n"
        "        // An armed eyedropper takes the click before any control does, which\n"
        "        // is the whole point of a mode: it samples what is under the pointer.\n"
        "        if self.eyedropper.active {\n"
        "            let color = self.color_under(x, y, size);\n"
        "            self.eyedrop_pick(x, y, color);\n"
        "            self.status = format!(\"Sampled {}\", color.to_hex6());\n"
        "            return Action::Redraw;\n"
        "        }\n"
        "        if self.show_help {\n"
        "            self.show_help = false;\n"
        "            return Action::Redraw;\n"
        "        }\n",
        [CARD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "colorpicker", timeout=900, only=only))
