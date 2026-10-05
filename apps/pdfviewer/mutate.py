"""Mutation test for pdfviewer.

Its keys: Ctrl chords are Ctrl chords, not AltGr; the print range and the
search type what a key typed; every other key is taken with nothing but Shift
held. And the list of keys is modal: while it is up, no key, press or turn of
the wheel reaches what it covers.

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

CARD = 'the_shortcut_card_takes_the_keys_and_a_press_rather_than_passing_them_on'

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        'AltGr is taken for Ctrl in the window',
        '            if key.pressed && textline::is_ctrl_chord(key.modifiers) {',
        '            if key.pressed && key.modifiers.ctrl {',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
    (
        'a chorded Escape puts the list of keys away',
        '            let closes = plain\n                && (matches!',
        '            let closes = true\n                && (matches!',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
    # The list of keys is modal for the keys and the pointer alike (known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        'a key reaches what the list of keys covers',
        '        if self.show_help {\n'
        '            let closes = plain\n'
        '                && (matches!(event.key, Key::F1 | Key::Escape)\n'
        '                    || event.key == Key::Slash && event.modifiers.shift);\n'
        '            if closes {\n'
        '                self.show_help = false;\n'
        '            }\n'
        '            return closes;\n'
        '        }\n',
        '',
        [CARD],
    ),
    (
        'F1 does not put the list of keys away',
        '                && (matches!(event.key, Key::F1 | Key::Escape)\n',
        '                && (matches!(event.key, Key::Escape)\n',
        [CARD],
    ),
    (
        '? does not put the list of keys away',
        '                    || event.key == Key::Slash && event.modifiers.shift);\n',
        ');\n',
        [CARD],
    ),
    (
        'a Ctrl chord acts on what the list of keys covers',
        '                    _ if self.show_help => {}\n',
        '',
        [CARD],
    ),
    (
        'the list of keys swallows the quit',
        '                    Key::Q => return Response::Exit,\n',
        '                    _ if self.show_help => {}\n'
        '                    Key::Q => return Response::Exit,\n',
        [CARD],
    ),
    (
        'a press goes through the list of keys',
        '                    MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                        self.show_help = false;\n'
        '                        return true;\n'
        '                    }\n',
        '',
        [CARD],
    ),
    (
        'only the left button puts the list of keys away',
        '                    MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n',
        '                    MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n',
        [CARD],
    ),
    (
        'a press under the list of keys leaves it up',
        '                        self.show_help = false;\n'
        '                        return true;\n',
        '                        return true;\n',
        [CARD],
    ),
    (
        'the wheel scrolls what the list of keys covers',
        '                    MouseEventKind::Scroll { .. } => return false,\n',
        '',
        [CARD],
    ),
    (
        'a chorded Escape closes the search',
        '        if event.key == Key::Escape && plain && self.search.active {',
        '        if event.key == Key::Escape && self.search.active {',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
    (
        'AltGr+O opens a file',
        '        if event.key == Key::O && textline::is_ctrl_chord(event.modifiers) {',
        '        if event.key == Key::O && event.modifiers.ctrl {',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
    (
        'a chord works the viewer',
        '        if !plain {\n            return false;\n        }\n',
        '',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
    (
        # What the boxes did before the editor: type whatever text a key
        # carried, a command's letter among it. Both go through `edit_box`.
        "a command's letter is typed into a box",
        '    if editor.text() != text.as_str() {\n        editor.set_text(text);\n    }\n',
        '    if key.types_text() && !textline::types_into_field(key) {\n'
        '        text.push_str(&key.text);\n'
        '        return Some(Edited {\n'
        '            text: true,\n'
        '            caret: false,\n'
        '        });\n'
        '    }\n'
        '    if editor.text() != text.as_str() {\n        editor.set_text(text);\n    }\n',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
    (
        'a chord works the print dialog',
        '            Key::Enter if plain => self.commit_print(),\n',
        '            Key::Enter => self.commit_print(),\n',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
]

# The search box and the page range box are the toolkit's field, edited by
# textline's editor (2026-10-04; lane C, c-e-a-theme-can-shape-the-controls).
SEARCH = "the_search_box_is_the_toolkits_field"
SEARCH_EDITS = "the_search_box_edits_like_a_field"
RANGE = "the_page_range_box_is_the_toolkits_field"

MUTATIONS += [
    (
        "the search box never has the keyboard",
        '            focused: self.search_focused && !self.show_help && !self.print_dialog.open,\n',
        '            focused: false,\n',
        [SEARCH],
    ),
    (
        "the search box keeps its mark under Print",
        '            focused: self.search_focused && !self.show_help && !self.print_dialog.open,\n',
        '            focused: self.search_focused && !self.show_help,\n',
        [SEARCH],
    ),
    (
        "a query that finds nothing is not red",
        '            invalid: !self.search.query.is_empty() && self.search.results.is_empty(),\n',
        '            invalid: false,\n',
        [SEARCH],
    ),
    (
        "a press leaves the caret where it was",
        '                        self.place_caret(target, &frame, drawn, *x);\n',
        '                        let _ = (&frame, drawn, *x);\n',
        [SEARCH_EDITS],
    ),
    (
        "the range box never has the keyboard",
        '            focused: self.print_dialog.open && !self.show_help,\n',
        '            focused: false,\n',
        [RANGE],
    ),
    (
        "a range of no pages is not red",
        '                && self.print_dialog.range() == PageRange::Custom(Vec::new()),\n',
        '                && false,\n',
        [RANGE],
    ),
    (
        "typing in the range box chooses no range",
        '                    if edited.text {\n                        self.print_dialog.choice = RangeChoice::Custom;\n',
        '                    if false {\n                        self.print_dialog.choice = RangeChoice::Custom;\n',
        [RANGE],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();\n',
        '        let _ = settings;\n',
        [SEARCH, RANGE],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "pdfviewer", timeout=600, only=only))
