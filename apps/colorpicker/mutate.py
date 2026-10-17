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
        "        self.editing && !self.show_help\n",
        "        false\n",
        [FIELD],
    ),
    (
        "the box is marked under the shortcut card",
        "        self.editing && !self.show_help\n",
        "        self.editing\n",
        [FIELD],
    ),
    (
        "a colour that would be refused is not red",
        "                invalid: !self.hex_input.is_empty()\n"
        "                    && PickedColor::from_hex_str(self.hex_input.text()).is_none(),\n",
        "                invalid: false,\n",
        [FIELD],
    ),
    (
        "an empty box starts out red",
        "                invalid: !self.hex_input.is_empty()\n"
        "                    && PickedColor::from_hex_str(self.hex_input.text()).is_none(),\n",
        "                invalid: PickedColor::from_hex_str(self.hex_input.text()).is_none(),\n",
        [FIELD],
    ),
    (
        "the caret shows under the shortcut card",
        "                    focused: has_keyboard,\n",
        "                    focused: self.editing,\n",
        [FIELD],
    ),
    (
        "under the card the box shows the colour, not what was typed",
        "        if self.editing {\n            let line = guitk::text::line_height(",
        "        if has_keyboard {\n            let line = guitk::text::line_height(",
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

# The value box is a textline LineBox, whose own table (apps/textline/mutate.py)
# holds the rows for its editing and drawing; these are colorpicker's: what a
# key types into it -- hex, upper-cased -- and a paste, and a chord being a
# chord. Alt+Escape closed the program, and AltGr+C copied the colour.
EDITS = "the_value_box_edits_at_a_caret"
PASTE = "a_paste_is_hex_or_nothing"
CHORD = "a_chord_is_neither_a_key_of_the_window_nor_typing"
SHOWN = "the_value_box_edits_the_hex_it_shows"
LONG_PRESS = "a_press_in_a_long_hex_lands_where_it_was_drawn"

MUTATIONS += [
    (
        "a letter that is not hex is typed",
        "                .filter(|c| hex(*c))\n",
        "",
        [EDITS],
    ),
    (
        "a hex digit is typed as it came",
        "                .map(|c| c.to_ascii_uppercase())\n",
        "",
        [EDITS],
    ),
    # No row for `textline::types_into_field(key)` made `key.types_text()`:
    # `typed()` leaves a command's letter out itself, so the two differ only
    # for a release, which `handle_key` drops before the box sees it.
    (
        "a paste that is not hex is mined for hex",
        "            if !pasted.is_empty() && pasted.chars().all(hex) {\n",
        "            if !pasted.is_empty() {\n                let pasted: String = pasted.chars().filter(|c| hex(*c)).collect();\n",
        [PASTE],
    ),
    (
        "Ctrl+V opens the box without the paste",
        "                self.begin_edit();\n                self.hex_input.select_all();\n                self.hex_key(key);\n",
        "                self.begin_edit();\n",
        [PASTE],
    ),
    (
        "a chord is a key of the window's",
        "        if !chord && !textline::is_plain(key.modifiers) {\n            return Action::None;\n        }\n",
        "",
        [CHORD],
    ),
    (
        "AltGr is Ctrl",
        "        let chord = textline::is_ctrl_chord(key.modifiers);\n",
        "        let chord = key.modifiers.ctrl;\n",
        [CHORD],
    ),
    (
        "a chord shuts the box",
        "        if textline::is_plain(key.modifiers) {\n            match key.key {\n                Key::Enter => return self.commit_edit(),\n",
        "        if true {\n            match key.key {\n                Key::Enter => return self.commit_edit(),\n",
        [CHORD],
    ),
    (
        "a press puts the caret at the start",
        "            x - rect.x - HEX_TEXT_INSET,\n",
        "            0.0,\n",
        [EDITS],
    ),
    (
        "a press in the open box does nothing",
        "                    self.press_hex(rect, x);\n",
        "                    let _ = (rect, x);\n",
        [EDITS, SHOWN],
    ),
    (
        "Copy puts nothing on the clipboard",
        "        guitk::clipboard::set_text(&copied);\n",
        "        let _ = &copied;\n",
        ["copy_to_clipboard", "ctrl_c_copies_the_value_the_active_tab_is_showing"],
    ),
    (
        "the box opens empty",
        "        self.hex_input = LineBox::holding(&self.current.to_hex6(), HEX_CAPACITY, FONT_SIZE);\n",
        "        self.hex_input = LineBox::holding(\"\", HEX_CAPACITY, FONT_SIZE);\n",
        ["the_value_box_takes_hex_digits_and_refuses_everything_else", EDITS],
    ),
    (
        "a press in the box with the keyboard is measured from the text's start",
        "            focused,\n        );\n",
        "            false,\n        );\n",
        [LONG_PRESS],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "colorpicker", timeout=900, only=only))
