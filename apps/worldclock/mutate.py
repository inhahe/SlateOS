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
        'Alt+Backspace deletes from the city search',
        '        Key::Backspace if !textline::is_alt_or_windows_chord(key.modifiers) => {',
        '        Key::Backspace => {',
        ['a_chord_is_neither_a_clock_key_nor_typing'],
    ),
    (
        "a chord works the picker's keys",
        '        _ if !plain && !textline::types_into_field(key) => return EventResult::Ignored,\n',
        '',
        ['a_chord_is_neither_a_clock_key_nor_typing'],
    ),
    (
        "the city search types a command's letter",
        '            if !textline::types_into_field(key) {\n                return EventResult::Ignored;\n            }\n',
        '',
        ['a_chord_is_neither_a_clock_key_nor_typing'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "worldclock", timeout=600, only=only))
