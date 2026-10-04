"""Mutation test for the colour picker.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The rows cover the shortcut card's hold on the pointer, and the value box
drawn by the toolkit as a field in the theme's shape (`guitk::field::draw`,
lane C's c-e-a-theme-can-shape-the-controls): what lights it, what marks it,
what makes it red.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

CARD = "the_shortcut_card_takes_a_press_rather_than_passing_it_on"
FIELD = "the_value_box_is_the_toolkits_field"
LEAVE = "a_pointer_that_leaves_the_window_ends_the_drag"

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
    # -- the value box, the toolkit's field
    (
        "the box is drawn the same wherever the pointer is",
        "                hovered: self.box_hovered && !self.show_help && !self.eyedropper.active,\n",
        "                hovered: false,\n",
        [FIELD],
    ),
    (
        "the box lights under the shortcut card",
        "                hovered: self.box_hovered && !self.show_help && !self.eyedropper.active,\n",
        "                hovered: self.box_hovered && !self.eyedropper.active,\n",
        [FIELD],
    ),
    (
        "the box lights with the eyedropper armed",
        "                hovered: self.box_hovered && !self.show_help && !self.eyedropper.active,\n",
        "                hovered: self.box_hovered && !self.show_help,\n",
        [FIELD],
    ),
    (
        "the open box is not marked",
        "        let has_keyboard = self.editing && !self.show_help;\n",
        "        let has_keyboard = false;\n",
        [FIELD],
    ),
    (
        "the box is marked under the shortcut card",
        "        let has_keyboard = self.editing && !self.show_help;\n",
        "        let has_keyboard = self.editing;\n",
        [FIELD],
    ),
    (
        "a colour that would be refused is not red",
        "                invalid: !self.hex_input.is_empty()\n"
        "                    && PickedColor::from_hex_str(&self.hex_input).is_none(),\n",
        "                invalid: false,\n",
        [FIELD],
    ),
    (
        "an empty box starts out red",
        "                invalid: !self.hex_input.is_empty()\n"
        "                    && PickedColor::from_hex_str(&self.hex_input).is_none(),\n",
        "                invalid: PickedColor::from_hex_str(&self.hex_input).is_none(),\n",
        [FIELD],
    ),
    (
        "the caret shows under the shortcut card",
        "        let text = if has_keyboard {\n",
        "        let text = if self.editing {\n",
        [FIELD],
    ),
    (
        "under the card the box shows the colour, not what was typed",
        "        } else if self.editing {\n"
        "            self.hex_input.clone()\n"
        "        } else {\n",
        "        } else {\n",
        [FIELD],
    ),
    (
        "the pointer is not followed",
        "        let over = self.hit_test(x, y, size) == Some(Target::ValueBox);\n",
        "        let over = false;\n",
        [FIELD],
    ),
    (
        "every move repaints",
        "        if over == self.box_hovered {\n"
        "            return Action::None;\n"
        "        }\n",
        "",
        [FIELD],
    ),
    (
        "leaving the window leaves the box lit",
        "        if std::mem::take(&mut self.box_hovered) {\n",
        "        if self.box_hovered {\n",
        [FIELD],
    ),
    (
        "a drag that ends as the pointer leaves does not repaint",
        "            return Action::Redraw;\n"
        "        }\n"
        "        ended\n",
        "            return Action::Redraw;\n"
        "        }\n"
        "        let _ = ended;\n"
        "        Action::None\n",
        [LEAVE],
    ),
    (
        "a drag does not hold the pointer",
        "            return self.drag_slider(channel, x, size);\n",
        "            self.drag_slider(channel, x, size);\n",
        [FIELD],
    ),
    (
        "the release does not light what the pointer is over",
        "                    let relit = self.handle_move(mouse.x, mouse.y, size);\n",
        "                    let relit = Action::None;\n",
        [FIELD],
    ),
    (
        "the box takes the toolkit's focus width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "colorpicker", timeout=900, only=only))
