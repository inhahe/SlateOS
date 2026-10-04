"""Mutation test for worldclock.

The keys: which are plain, and which are typing.

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
        '    if plain && (key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift)) {',
        '    if key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift) {',
        ['a_chord_is_neither_a_clock_key_nor_typing'],
    ),
    (
        'a chord puts the list of keys away',
        '        if plain && matches!(key.key, Key::Escape | Key::Enter | Key::Space) {',
        '        if matches!(key.key, Key::Escape | Key::Enter | Key::Space) {',
        ['a_chord_is_neither_a_clock_key_nor_typing'],
    ),
    (
        "a chord works the clocks' keys",
        '    if !plain {\n        return EventResult::Ignored;\n    }\n',
        '',
        ['a_chord_is_neither_a_clock_key_nor_typing'],
    ),
    (
        # What the search did before the editor: Backspace with any chord.
        'Alt+Backspace deletes from the city search',
        '        if self.picker_editor.text() != self.picker_search {\n'
        '            self.picker_editor.set_text(&self.picker_search);\n'
        '        }\n',
        '        if key.key == Key::Backspace {\n'
        '            self.picker_search.pop();\n'
        '            return EventResult::Consumed;\n'
        '        }\n'
        '        if self.picker_editor.text() != self.picker_search {\n'
        '            self.picker_editor.set_text(&self.picker_search);\n'
        '        }\n',
        ['a_chord_is_neither_a_clock_key_nor_typing'],
    ),
    (
        "a chord works the picker's keys",
        '        Key::Escape if plain => state.show_picker = false,\n',
        '        Key::Escape => state.show_picker = false,\n',
        ['a_chord_is_neither_a_clock_key_nor_typing'],
    ),
    (
        # What the search did before the editor: type whatever text a key
        # carried, a command's letter among it.
        "the city search types a command's letter",
        '        if self.picker_editor.text() != self.picker_search {\n'
        '            self.picker_editor.set_text(&self.picker_search);\n'
        '        }\n',
        '        if key.types_text() && !textline::types_into_field(key) {\n'
        '            self.picker_search.push_str(&key.text);\n'
        '            return EventResult::Consumed;\n'
        '        }\n'
        '        if self.picker_editor.text() != self.picker_search {\n'
        '            self.picker_editor.set_text(&self.picker_search);\n'
        '        }\n',
        ['a_chord_is_neither_a_clock_key_nor_typing'],
    ),
    # The city search is the toolkit's field, edited by textline's editor
    # (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
    (
        "the city search never has the keyboard",
        '            focused: self.show_picker && !self.show_help,\n',
        '            focused: false,\n',
        ['the_city_search_is_the_toolkits_field'],
    ),
    (
        "the city search keeps its mark under the list of keys",
        '            focused: self.show_picker && !self.show_help,\n',
        '            focused: self.show_picker,\n',
        ['the_city_search_is_the_toolkits_field'],
    ),
    (
        "a search that finds nothing is not red",
        '            invalid: !self.picker_search.is_empty() && self.filtered_timezones().is_empty(),\n',
        '            invalid: false,\n',
        ['the_city_search_is_the_toolkits_field'],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();\n',
        '        let _ = settings;\n',
        ['the_city_search_is_the_toolkits_field'],
    ),
    (
        "the caret is drawn at the start of the search",
        '                        self.picker_editor.cursor()\n',
        '                        text::TextCursor::default()\n',
        ['the_city_search_is_the_toolkits_field'],
    ),
    (
        "a search is folded as ASCII",
        '        let query = self.picker_search.to_lowercase();\n',
        '        let query = self.picker_search.to_ascii_lowercase();\n',
        ['a_search_in_capitals_finds_a_city_with_an_accent'],
    ),
]

CARD = 'the_shortcut_card_takes_a_press_rather_than_passing_it_on'

MUTATIONS += [
    # The list of keys takes the pointer (2026-10-04; known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        'a press goes through the list of keys',
        '    if state.show_help {\n'
        '        return match mouse.kind {\n'
        '            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                state.show_help = false;\n'
        '                EventResult::Consumed\n'
        '            }\n'
        '            _ => EventResult::Ignored,\n'
        '        };\n'
        '    }\n',
        '',
        [CARD],
    ),
    (
        'only the left button puts the list of keys away',
        '            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                state.show_help = false;\n',
        '            MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n'
        '                state.show_help = false;\n',
        [CARD],
    ),
    (
        'a press under the list of keys leaves it up',
        '                state.show_help = false;\n'
        '                EventResult::Consumed\n'
        '            }\n'
        '            _ => EventResult::Ignored,\n',
        '                EventResult::Consumed\n'
        '            }\n'
        '            _ => EventResult::Ignored,\n',
        [CARD],
    ),
    (
        'the wheel scrolls what the list of keys covers',
        '    if state.show_help {\n'
        '        return match mouse.kind {\n',
        '    if state.show_help && !matches!(mouse.kind, MouseEventKind::Scroll { .. }) {\n'
        '        return match mouse.kind {\n',
        [CARD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "worldclock", timeout=600, only=only))
