"""Mutation test for musicplayer.

Its keys: F1's list stays up past the release; Ctrl+O, S and F are Ctrl chords,
not AltGr; the search types what a key typed; every other key is taken with
nothing but Shift held.

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
        'a release is a keystroke',
        '    if !key_event.pressed {\n        return false;\n    }\n    // Every key but a Ctrl chord',
        '    // Every key but a Ctrl chord',
        ['f1_raises_the_keys_and_a_chord_is_neither_a_player_key_nor_typing'],
    ),
    (
        'a chord raises the keys',
        '    if key_event.key == Key::F1 && plain {',
        '    if key_event.key == Key::F1 {',
        ['f1_raises_the_keys_and_a_chord_is_neither_a_player_key_nor_typing'],
    ),
    (
        "a command's letter is typed into the search",
        '        if textline::types_into_field(key_event) {',
        '        if key_event.types_text() {',
        ['f1_raises_the_keys_and_a_chord_is_neither_a_player_key_nor_typing'],
    ),
    (
        'a chord works the search box',
        '        if !plain {\n            return false;\n        }\n        match key_event.key {\n            Key::Escape => {',
        '        match key_event.key {\n            Key::Escape => {',
        ['f1_raises_the_keys_and_a_chord_is_neither_a_player_key_nor_typing'],
    ),
    (
        'AltGr is taken for Ctrl',
        '    if textline::is_ctrl_chord(key_event.modifiers) {',
        '    if key_event.modifiers.ctrl {',
        ['f1_raises_the_keys_and_a_chord_is_neither_a_player_key_nor_typing'],
    ),
    (
        'a chord works the player',
        '    if !plain {\n        return false;\n    }\n\n    // Global keyboard shortcuts',
        '    // Global keyboard shortcuts',
        ['f1_raises_the_keys_and_a_chord_is_neither_a_player_key_nor_typing'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "musicplayer", timeout=600, only=only))
