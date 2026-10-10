"""Mutation test for startupmanager.

The keys: which are plain, which are Ctrl chords, and which are typing.  And,
since 2026-10-04, the list of keys' hold on the pointer: a press with it up
puts it away and reaches nothing under it, and the wheel scrolls nothing it
covers.

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
        'a chord raises the list of keys',
        '        if key.key == Key::F1 && plain {',
        '        if key.key == Key::F1 {',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chorded Escape puts the list of keys away',
        '            if plain && matches!(key.key, Key::Escape | Key::Enter) {',
        '            if matches!(key.key, Key::Escape | Key::Enter) {',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chorded Escape answers the question before a removal',
        '                Key::Escape if plain => {\n                    self.close_dialog();',
        '                Key::Escape => {\n                    self.close_dialog();',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chorded Enter removes the entry',
        '                Key::Enter if plain => {\n                    self.confirm_delete();',
        '                Key::Enter => {\n                    self.confirm_delete();',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chorded Escape closes the dialog',
        '            Key::Escape if plain => {\n                self.close_dialog();',
        '            Key::Escape => {\n                self.close_dialog();',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        'a chorded Enter saves the dialog',
        '            Key::Enter if plain => {\n                self.save_dialog();',
        '            Key::Enter => {\n                self.save_dialog();',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        "Alt+Tab walks the dialog's fields",
        '            Key::Tab | Key::Down if plain => dlg.focus_next(),',
        '            Key::Tab | Key::Down => dlg.focus_next(),',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        "a chorded Up walks the dialog's fields back",
        '            Key::Up if plain => dlg.focus_prev(),',
        '            Key::Up => dlg.focus_prev(),',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    # No rows for "the Windows key's Backspace deletes in the dialog" or
    # "the dialog types a command's letter", nor for the search's two below:
    # since 2026-10-04 the boxes' keys are textline::apply_key's, which makes
    # those distinctions itself, in its own crate and with its own tests;
    # a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types still holds
    # both boxes to them.
    (
        'AltGr is taken for Ctrl in the list',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return match key.key {',
        '        if key.modifiers.ctrl {\n            return match key.key {',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        "a chord works the list's keys",
        '        if !textline::is_plain(key.modifiers) {\n            return EventResult::Ignored;\n        }\n',
        '',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        'AltGr+Q closes the window',
        '            && textline::is_ctrl_chord(key.modifiers)\n        {',
        '            && key.modifiers.ctrl\n        {',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
]

CARD = 'the_shortcut_card_takes_a_press_rather_than_passing_it_on'
TOUCHPAD = 'a_touchpads_small_turns_add_up_to_rows'
FRACTION = 'a_fraction_of_a_notch_does_not_outlive_the_rows'
FIELDS = 'the_text_boxes_are_the_toolkits_fields'

MUTATIONS += [
    # The text boxes are the toolkit's fields (2026-10-04; lane C,
    # c-e-a-theme-can-shape-the-controls).
    (
        'the search box is drawn in no state',
        '            self.field_state(Target::Search),\n',
        '            guitk::field::State::default(),\n',
        [FIELDS],
    ),
    (
        "the dialog's boxes are drawn in no state",
        '            let state = self.field_state(Target::DialogField(i));\n',
        '            let state = guitk::field::State::default();\n',
        [FIELDS],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();\n',
        '        let _ = settings;\n',
        [FIELDS],
    ),
    (
        'a box shows through the list of keys',
        '        let open = open && !self.show_help;\n',
        '        let open = open || self.show_help;\n',
        [FIELDS],
    ),
    (
        'the search box shows through a dialog',
        '            (Target::Search, DialogState::Closed) => (true, self.search_focused),\n',
        '            (Target::Search, _) => (true, self.search_focused),\n',
        [FIELDS],
    ),
    (
        "every one of the dialog's boxes is marked",
        '            (Target::DialogField(i), DialogState::AddEdit(dlg)) => (true, dlg.focused_field == i),\n',
        '            (Target::DialogField(_), DialogState::AddEdit(_)) => (true, true),\n',
        [FIELDS],
    ),
    (
        'the pointer is never followed',
        '        self.hover = over;\n',
        '        let _ = over;\n',
        [FIELDS],
    ),
    (
        'the light stays after the pointer leaves',
        '            MouseEventKind::Leave => Option::None,\n',
        '            MouseEventKind::Leave => self.hover,\n',
        [FIELDS],
    ),
]

MUTATIONS += [
    # The table's wheel adds a touchpad's small turns up (2026-10-04).
    (
        "each event's rows are rounded on their own",
        '                let rows = self.table_wheel.rows(dy);\n',
        '                let rows = wheel::rows_f(dy).round() as isize;\n',
        [TOUCHPAD],
    ),
    (
        'a turn that moves nothing says it moved',
        '                if self.scroll_offset == before {\n',
        '                if false {\n',
        [TOUCHPAD],
    ),
    (
        'a fraction of a notch outlives the rows',
        '        self.table_wheel.reset();\n',
        '',
        [FRACTION],
    ),
]

MUTATIONS += [
    # The list of keys takes the pointer (2026-10-04; known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        'a press goes through the list of keys',
        '            Event::Mouse(mouse) if self.show_help => match mouse.kind {\n'
        '                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                    self.show_help = false;\n'
        '                    EventResult::Consumed\n'
        '                }\n'
        '                _ => EventResult::Ignored,\n'
        '            },\n',
        '',
        [CARD],
    ),
    (
        'only the left button puts the list of keys away',
        '                MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                    self.show_help = false;\n',
        '                MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n'
        '                    self.show_help = false;\n',
        [CARD],
    ),
    (
        'a press under the list of keys leaves it up',
        '                    self.show_help = false;\n'
        '                    EventResult::Consumed\n',
        '                    EventResult::Consumed\n',
        [CARD],
    ),
    (
        'the wheel scrolls what the list of keys covers',
        '                _ => EventResult::Ignored,\n'
        '            },\n'
        '            Event::Mouse(mouse) => {\n',
        '                MouseEventKind::Scroll { dy, .. } => self.handle_scroll(mouse.x, mouse.y, dy),\n'
        '                _ => EventResult::Ignored,\n'
        '            },\n'
        '            Event::Mouse(mouse) => {\n',
        [CARD],
    ),
]

# The boxes edit at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): they
# took typing at their end and Backspace from it, and nothing else; and Home,
# End and Delete went to the table under the search box -- Delete removed the
# selected entry while a search was being typed.
SEARCH_EDITS = "the_search_box_edits_at_a_caret"
FIELD_EDITS = "the_dialog_fields_edit_at_a_caret"

MUTATIONS += [
    (
        "the table's keys come before the search box's",
        "        if self.search_focused\n            && let Some(changed) = self.box_key(Target::Search, key)\n        {\n",
        "        if false\n            && let Some(changed) = self.box_key(Target::Search, key)\n        {\n",
        [SEARCH_EDITS],
    ),
    (
        "an edit of the search leaves the table scrolled",
        "            if changed {\n                self.restart_table();\n            }\n",
        "",
        ["an_edit_of_the_search_shows_its_rows_from_the_top"],
    ),
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.clipboard = copied;\n",
        "            let _ = copied;\n",
        [SEARCH_EDITS],
    ),
    (
        "the editor is kept for the box the keyboard moved to",
        "        if self.editor_for != Some(target) || self.editor.text() != self.box_text(target) {\n",
        "        if self.editor.text() != self.box_text(target) {\n",
        ["a_dialog_field_moved_to_is_edited_from_its_end"],
    ),
    (
        "a press puts the caret at the start",
        "            x - rect.x - inset,\n",
        "            0.0,\n",
        [SEARCH_EDITS, FIELD_EDITS],
    ),
    (
        "a press in a dialog field does not place the caret",
        "                    self.press_box(field, rect, FIELD_TEXT_INSET, x);\n",
        "                    let _ = (field, rect);\n",
        [FIELD_EDITS],
    ),
    (
        "Ctrl+F does not select what the search box holds",
        "        self.editor.select_all();\n",
        "",
        [SEARCH_EDITS],
    ),
    (
        "the caret is drawn at the end",
        "            (self.editor.cursor(), self.editor.selection_anchor())\n",
        "            (text::TextCursor::from(held.len()), self.editor.selection_anchor())\n",
        [SEARCH_EDITS],
    ),
    (
        "a field with the keyboard draws no caret",
        "                state\n                    .focused\n                    .then(|| self.box_caret(Target::DialogField(i))),\n",
        "                None,\n",
        [FIELD_EDITS],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "startupmanager", timeout=600, only=only))
