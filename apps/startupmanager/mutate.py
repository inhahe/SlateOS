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
    (
        "the Windows key's Backspace deletes in the dialog",
        '            Key::Backspace if !textline::is_alt_or_windows_chord(key.modifiers) => {\n                dlg.focused_text_mut().pop();',
        '            Key::Backspace => {\n                dlg.focused_text_mut().pop();',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        "the dialog types a command's letter",
        '                if textline::types_into_field(key) {\n                    dlg.focused_text_mut().extend(key.typed());',
        '                if key.types_text() {\n                    dlg.focused_text_mut().extend(key.typed());',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        'AltGr is taken for Ctrl in the list',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return match key.key {',
        '        if key.modifiers.ctrl {\n            return match key.key {',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        "the search types a command's letter",
        '        if self.search_focused && textline::types_into_field(key) {',
        '        if self.search_focused && key.types_text() {',
        ['a_chord_is_neither_a_manager_key_nor_typing_and_altgr_types'],
    ),
    (
        'Alt+Backspace deletes from the search',
        '            && !textline::is_alt_or_windows_chord(key.modifiers)\n        {',
        '        {',
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

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "startupmanager", timeout=600, only=only))
