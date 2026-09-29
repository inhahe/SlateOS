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
    (
        "a command's letter is typed into a pattern",
        '            if textline::types_into_field(key) {',
        '            if key.types_text() {',
        ['a_chord_is_neither_a_key_of_the_window_nor_typing'],
    ),
    (
        'a chord works the pattern field',
        '                _ if !plain => {}\n',
        '',
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
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "defrag", timeout=600, only=only))
