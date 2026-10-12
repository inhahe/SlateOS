"""Mutation test for clipmanager.

Its keys: Ctrl+S and Ctrl+O are Ctrl chords, not AltGr, and reach it from a
field; a field types what a key typed and no command's letter; the list's keys
and the fields' own are taken with nothing but Shift held.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / 'src' / 'main.rs'

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        'a chord raises the keys',
        '        if key.key == Key::F1 && plain {',
        '        if key.key == Key::F1 {',
        ['a_chord_is_neither_a_key_of_the_list_nor_typing_and_altgr_types'],
    ),
    (
        'AltGr is taken for Ctrl',
        '        if textline::is_ctrl_chord(key.modifiers) {',
        '        if key.modifiers.ctrl {',
        ['a_chord_is_neither_a_key_of_the_list_nor_typing_and_altgr_types'],
    ),
    (
        'a chord works the list',
        '        if !plain {\n            return Action::None;\n        }\n',
        '',
        ['a_chord_is_neither_a_key_of_the_list_nor_typing_and_altgr_types'],
    ),
    # No row for "a command's letter is typed into a field": since 2026-10-04
    # the boxes' typing is textline::apply_key's, which tells a command from
    # AltGr itself, in its own crate and with its own tests;
    # a_chord_is_neither_a_key_of_the_list_nor_typing_and_altgr_types still
    # holds the boxes to it.
    (
        "a chord works a field's own keys",
        '        if textline::is_plain(key.modifiers) {\n            match key.key {\n                Key::Escape => {\n                    self.focus = None;',
        '        if true {\n            match key.key {\n                Key::Escape => {\n                    self.focus = None;',
        ['a_chord_is_neither_a_key_of_the_list_nor_typing_and_altgr_types'],
    ),
    # -- the text boxes, the toolkit's fields (c-e-a-theme-can-shape-the-controls)
    (
        "a text box is drawn the same wherever the pointer is",
        '                hovered: self.hover == Some(target),',
        '                hovered: false,',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        "the search box is not marked while it has the keyboard",
        '        .field_look(Target::SearchBox, focused)',
        '        .field_look(Target::SearchBox, false)',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        "the tag entry is not marked while it has the keyboard",
        '            state.field_look(Target::TagField, state.focus == Some(Field::Tag)),',
        '            state.field_look(Target::TagField, false),',
        ['the_tag_entry_is_the_toolkits_field'],
    ),
    (
        "the pointer is not followed",
        '                    let over = self.hit_test(mouse.x, mouse.y, size);',
        '                    let over = None::<Target>;',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        "the pointer leaving the window leaves its box lit",
        '                    if self.hover.take().is_some() {',
        '                    if self.hover.is_some() {',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    (
        "the text boxes take the toolkit's focus width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();',
        '        let _ = settings;',
        ['the_text_boxes_are_the_toolkits_fields'],
    ),
    # -- the shortcut card's hold on the pointer
    (
        "a press goes through the shortcut card",
        '                MouseEventKind::Press(_) if self.show_help => {\n'
        '                    self.show_help = false;\n'
        '                    Action::Redraw\n'
        '                }\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        "only the left button puts the card away",
        '                MouseEventKind::Press(_) if self.show_help => {\n',
        '                MouseEventKind::Press(MouseButton::Left) if self.show_help => {\n',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        "the wheel scrolls what the card covers",
        '                MouseEventKind::Scroll { .. } if self.show_help => Action::None,\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
]

# The boxes edit at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): they
# took typing at their end and Backspace from it, and nothing else, with a `_`
# typed onto the end for a caret.
EDITS = "the_search_box_edits_at_a_caret"
TEMPLATE = "a_templates_boxes_edit_at_a_caret"
SHOWN = "a_box_edits_the_text_it_shows"

MUTATIONS += [
    (
        "Ctrl+A, C, X and V are nobody's",
        "                _ => {\n"
        "                    return match self.focus.and_then(|field| self.box_key(field, key)) {\n",
        "                _ => {\n"
        "                    return match None::<bool> {\n",
        [EDITS],
    ),
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "a search edited is not filtered by",
        "            if field == Field::Search {\n"
        "                self.scroll_offset = 0;\n"
        "                self.refresh_filter();\n"
        "            }\n"
        "        }\n"
        "        let editor = &self.editor;\n",
        "        }\n"
        "        let editor = &self.editor;\n",
        ["typing_in_the_search_box_narrows_the_list_and_backspacing_widens_it"],
    ),
    (
        "the editor is kept for the box the keyboard moved to",
        "        if self.editor_for != Some(field) || self.editor.text() != self.box_value(field) {\n",
        "        if self.editor.text() != self.box_value(field) {\n",
        [SHOWN],
    ),
    (
        "a key finds the editor holding another text",
        "        if self.editor_for != Some(field) || self.editor.text() != self.box_value(field) {\n",
        "        if self.editor_for != Some(field) {\n",
        [SHOWN],
    ),
    (
        "the caret is drawn at the start",
        "        let (cursor, selection_anchor) = if focused {\n            text.caret\n",
        "        let (cursor, selection_anchor) = if false {\n            text.caret\n",
        [EDITS],
    ),
    (
        "an empty box with the keyboard draws no caret",
        "        if focused {\n            textedit::push_caret(\n",
        "        if false {\n            textedit::push_caret(\n",
        [TEMPLATE],
    ),
    (
        "a press puts the caret at the start",
        "            x - area.x,\n",
        "            0.0,\n",
        [EDITS, TEMPLATE],
    ),
    (
        "a press in a box does not place the caret",
        "            self.press_box(field, rect, drawn, x);\n",
        "            let _ = (field, rect, drawn);\n",
        [EDITS, TEMPLATE],
    ),
    (
        "the search's text is drawn over its label",
        "            Self::Search => (72.0, 128.0),\n",
        "            Self::Search => (6.0, 128.0),\n",
        ["the_search_text_clears_its_label"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "clipmanager", timeout=600, only=only))
