"""Mutation test for podcast.

Its keys: Ctrl+S and Ctrl+O are Ctrl chords, not AltGr; the named keys are
taken with nothing but Shift held; a letter counts only as a character typed.

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
        '        if plain && (event.key == Key::F1 || (event.key == Key::Slash && event.modifiers.shift)) {',
        '        if event.key == Key::F1 || (event.key == Key::Slash && event.modifiers.shift) {',
        ['a_chord_is_neither_a_player_key_nor_its_letter'],
    ),
    (
        'AltGr is taken for Ctrl',
        '        if textline::is_ctrl_chord(event.modifiers) {',
        '        if event.modifiers.ctrl {',
        ['a_chord_is_neither_a_player_key_nor_its_letter'],
    ),
    (
        'a chord works the named keys and its letter',
        '        if !plain {\n            return textline::types_into_field(event) && self.handle_typed(event);\n        }\n',
        '',
        ['a_chord_is_neither_a_player_key_nor_its_letter'],
    ),
    (
        "a chord's letter is a shortcut",
        '            return textline::types_into_field(event) && self.handle_typed(event);',
        '            return self.handle_typed(event);',
        ['a_chord_is_neither_a_player_key_nor_its_letter'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "podcast", timeout=600, only=only))
