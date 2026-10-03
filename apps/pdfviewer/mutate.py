"""Mutation test for pdfviewer.

Its keys: Ctrl chords are Ctrl chords, not AltGr; the print range and the
search type what a key typed; every other key is taken with nothing but Shift
held.

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
        'AltGr is taken for Ctrl in the window',
        '            if key.pressed && textline::is_ctrl_chord(key.modifiers) {',
        '            if key.pressed && key.modifiers.ctrl {',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
    (
        'a chorded Escape puts the list of keys away',
        '        if event.key == Key::Escape && plain && self.show_help {',
        '        if event.key == Key::Escape && self.show_help {',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
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
        "a command's letter is typed into the print range",
        '        if textline::types_into_field(event) {\n            self.print_dialog.range_text',
        '        if event.types_text() {\n            self.print_dialog.range_text',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
    (
        'a chord works the print dialog',
        '        if !textline::is_plain(event.modifiers) {\n            return false;\n        }\n        match event.key {\n            Key::Escape => {\n                self.print_dialog.open = false;',
        '        match event.key {\n            Key::Escape => {\n                self.print_dialog.open = false;',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
    (
        "a command's letter is typed into the search",
        '        if textline::types_into_field(event) {\n            self.search.query',
        '        if event.types_text() {\n            self.search.query',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
    (
        'a chord works the search box',
        '        if !textline::is_plain(event.modifiers) {\n            return false;\n        }\n        match event.key {\n            Key::Enter => {\n                self.search.next_match();',
        '        match event.key {\n            Key::Enter => {\n                self.search.next_match();',
        ['a_chord_is_neither_a_viewer_key_nor_typing'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "pdfviewer", timeout=600, only=only))
