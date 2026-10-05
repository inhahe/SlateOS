"""Mutation test for the disk imager's Create tab.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: the output format was drawn from
`CreateOptions::format`, which offered Raw, ISO and Gzip-compressed and which
nothing set and nothing branched on -- right by coincidence, and a trap the day
a picker set it.  It is drawn from `CREATED_FORMAT`, what the copy writes.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

FORMAT = "the_create_tab_names_the_format_the_copy_writes"
CARD = "the_shortcut_card_takes_every_key_and_press_while_it_is_up"

MUTATIONS = [
    (
        "the Create tab names a format the copy does not write",
        "const CREATED_FORMAT: ImageFormat = ImageFormat::Raw;",
        "const CREATED_FORMAT: ImageFormat = ImageFormat::Iso9660;",
        [FORMAT],
    ),
    (
        "the Create tab names no format",
        "                CREATED_FORMAT.name(),",
        '                "",',
        [FORMAT],
    ),
    (
        'AltGr is taken for Ctrl',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return match key.key {',
        '        if key.modifiers.ctrl {\n            return match key.key {',
        ['a_key_held_with_a_modifier_is_not_the_windows_and_altgr_is_not_ctrl'],
    ),
    (
        "a key held with Alt or the Windows key is the window's",
        '        if !textline::is_plain(key.modifiers) {\n            return EventResult::Ignored;\n        }\n',
        '',
        ['a_key_held_with_a_modifier_is_not_the_windows_and_altgr_is_not_ctrl'],
    ),
    # -- the shortcut card is modal, for the keys and the pointer
    (
        "the card is modal for nothing",
        '        if self.show_help {\n            match event {\n',
        '        if false && self.show_help {\n            match event {\n',
        [CARD],
    ),
    (
        "a key that is not the card's acts behind it",
        '                        self.show_help = false;\n'
        '                    }\n'
        '                    return EventResult::Consumed;\n',
        '                        self.show_help = false;\n'
        '                        return EventResult::Consumed;\n'
        '                    }\n',
        [CARD],
    ),
    (
        "Escape does not put the card away",
        '                    if textline::is_plain(key.modifiers) && matches!(key.key, Key::F1 | Key::Escape)\n',
        '                    if textline::is_plain(key.modifiers) && matches!(key.key, Key::F1)\n',
        [CARD],
    ),
    (
        "a press goes through the card",
        '                    MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                        self.show_help = false;\n'
        '                        return EventResult::Consumed;\n'
        '                    }\n',
        '',
        [CARD],
    ),
    (
        "only the left button puts the card away",
        '                    MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n',
        '                    MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n',
        [CARD],
    ),
    (
        "the wheel scrolls what the card covers",
        '                    MouseEventKind::Scroll { .. } => return EventResult::Ignored,\n',
        '',
        [CARD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "diskimager", timeout=900, only=only))
