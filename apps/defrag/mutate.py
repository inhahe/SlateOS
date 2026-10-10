"""Mutation test for defrag.

Its keys: a chord with Ctrl, Alt or the Windows key is not the window's --
Alt+Enter answered the SSD warning -- the pattern field types what a key typed,
and Ctrl+Q is a Ctrl chord, not AltGr.

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
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    (
        'a chord answers the SSD warning',
        '                _ if !plain => EventResult::Consumed,\n',
        '',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    # No row for "a command's letter is typed into a pattern": since
    # 2026-10-04 the box's typing is textline::apply_key's, which tells a
    # command from AltGr itself, in its own crate and with its own tests;
    # a_chord_is_neither_a_key_of_the_window_nor_typing still holds the box
    # to it.
    (
        'a chord works the pattern field',
        '                Key::Escape if plain => {\n',
        '                Key::Escape => {\n',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    (
        'a chord changes the view',
        '        if key.key == Key::Tab && plain {',
        '        if key.key == Key::Tab {',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    (
        'AltGr+Q closes the window',
        '            && textline::is_ctrl_chord(key.modifiers)',
        '            && key.modifiers.ctrl',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    # -- the shortcut card's hold on the pointer
    (
        'a press goes through the shortcut card',
        '            MouseEventKind::Press(_) if ui.show_help => {\n'
        '                ui.show_help = false;\n'
        '                EventResult::Consumed\n'
        '            }\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'only the left button puts the card away',
        '            MouseEventKind::Press(_) if ui.show_help => {\n',
        '            MouseEventKind::Press(MouseButton::Left) if ui.show_help => {\n',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'the wheel scrolls what the card covers',
        '            MouseEventKind::Scroll { .. } if ui.show_help => EventResult::Ignored,\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    # -- the clock, 2026-10-03: none was asked for, so a defrag never moved
    (
        'a running defrag asks for no clock',
        '        (self.defrag_state() == DefragState::Running).then_some(TICK)\n',
        '        None\n',
        ['a_running_defrag_asks_for_the_clock_and_nothing_else_does'],
    ),
    (
        'an idle window asks for the clock',
        '        (self.defrag_state() == DefragState::Running).then_some(TICK)\n',
        '        Some(TICK)\n',
        ['a_running_defrag_asks_for_the_clock_and_nothing_else_does'],
    ),
]

FIELD = 'the_exclude_box_is_the_toolkits_field'

MUTATIONS += [
    # The exclude pattern's box is the toolkit's field (2026-10-04; lane C,
    # c-e-a-theme-can-shape-the-controls).
    (
        'the box never lights',
        '            hovered: open && self.exclude_input_hovered,\n',
        '            hovered: false,\n',
        [FIELD],
    ),
    (
        'the box is never marked',
        '            focused: open && self.show_exclude_editor,\n',
        '            focused: false,\n',
        [FIELD],
    ),
    (
        'the box shows through the list of keys and the SSD question',
        '        let open = !self.show_help && !self.show_ssd_warning;\n',
        '        let open = true;\n',
        [FIELD],
    ),
    (
        'the light stays after the pointer leaves',
        '            MouseEventKind::Leave => ui.point_at(None),\n',
        '',
        [FIELD],
    ),
    (
        'a change in the light asks for no repaint',
        '        if over == self.exclude_input_hovered {\n',
        '        if true {\n',
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();\n',
        '        let _ = settings;\n',
        [FIELD],
    ),
]

# The pattern box edits at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): it
# took typing at its end and Backspace from it, and nothing else, and its
# caret was a `|` typed onto the pattern's end.
EDITS = "the_pattern_box_edits_at_a_caret"
SHOWN = "the_pattern_box_edits_the_pattern_it_shows"

MUTATIONS += [
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.exclude_clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "a key finds the editor holding another pattern",
        "    fn exclude_key(&mut self, key: &KeyEvent) {\n        if self.exclude_editor.text() != self.exclude_input {\n",
        "    fn exclude_key(&mut self, key: &KeyEvent) {\n        if false {\n",
        [SHOWN],
    ),
    (
        "a press finds the editor holding another pattern",
        "        let drawn = self.exclude_cursor();\n        if self.exclude_editor.text() != self.exclude_input {\n",
        "        let drawn = self.exclude_cursor();\n        if false {\n",
        [SHOWN],
    ),
    (
        "a press puts the caret at the start",
        "            x - rect.x - EXCLUDE_TEXT_INSET,\n",
        "            0.0,\n",
        [EDITS],
    ),
    (
        "a press in the box does nothing",
        "            Target::ExcludeInput => self.press_exclude(x),\n",
        "            Target::ExcludeInput => {}\n",
        [EDITS, SHOWN],
    ),
    (
        "the caret is drawn at the start",
        "                    cursor: self.exclude_cursor(),\n",
        "                    cursor: text::TextCursor::default(),\n",
        [EDITS],
    ),
    (
        "the selection is not drawn",
        "                    selection_anchor: if self.exclude_editor.text() == self.exclude_input {\n",
        "                    selection_anchor: if false {\n",
        [EDITS],
    ),
    (
        "the box with the keys draws no caret",
        "                    focused: state.focused,\n                    x: card_x + PADDING + EXCLUDE_TEXT_INSET,\n",
        "                    focused: false,\n                    x: card_x + PADDING + EXCLUDE_TEXT_INSET,\n",
        [EDITS],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "defrag", timeout=600, only=only))
