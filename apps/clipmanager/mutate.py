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
    (
        "a command's letter is typed into a field",
        '        if textline::types_into_field(key) {',
        '        if key.types_text() {',
        ['a_chord_is_neither_a_key_of_the_list_nor_typing_and_altgr_types'],
    ),
    (
        "a chord works a field's own keys",
        '        if !textline::is_plain(key.modifiers) {\n            return Action::None;\n        }\n',
        '',
        ['a_chord_is_neither_a_key_of_the_list_nor_typing_and_altgr_types'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "clipmanager", timeout=600, only=only))
