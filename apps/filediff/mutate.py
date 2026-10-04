"""Mutation test for the file comparer's folder comparison.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: the folder view was a model and a painter
that nothing could reach -- no key entered it, and its comparison was over
names and texts held in memory.  Ctrl+D compares two real folders now, byte
for byte within a budget, and the list opens a pair into the file view.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

FOLDERS = "two_real_folders_are_compared_file_by_file"
BYTES = "same_size_different_bytes_is_different_and_the_budget_is_honest"
PAIR = "enter_opens_the_pair_and_escape_goes_back_to_the_folders"
CTRL_D = "ctrl_d_asks_for_the_left_folder_then_the_right"

MUTATIONS = [
    (
        "Ctrl+D compares nothing",
        "            Key::D => {\n                self.choose_folders();",
        "            Key::D if false => {\n                self.choose_folders();",
        [CTRL_D],
    ),
    (
        "different bytes are called the same",
        "        if ba.get(..n) != Some(&*chunk_b) {",
        "        if false {",
        [BYTES],
    ),
    (
        "sizes are not compared first",
        "    if ma.len() != mb.len() {\n        return Ok(Some(false));\n    }",
        "",
        [BYTES],
    ),
    (
        "the budget is ignored",
        "    if ma.len() > *budget {\n        return Ok(None);\n    }",
        "",
        [BYTES],
    ),
    (
        "a one-sided folder lists everything in it",
        "        if one_sided.iter().any(|dir| rel.starts_with(dir)) {\n            continue;\n        }",
        "",
        [FOLDERS],
    ),
    (
        "Enter opens nothing",
        "            Key::Enter => Some(self.open_folder_entry()),",
        "            Key::Enter => Some(EventResult::Consumed),",
        [PAIR],
    ),
    (
        "Escape does not go back to the folders",
        "        if key.key == Key::Escape && plain && self.from_folders && self.dir_compare.is_some() {",
        "        if key.key == Key::Escape && plain && self.from_folders && self.dir_compare.is_some() && false {",
        [PAIR],
    ),
    (
        'a chord raises the keys',
        '        if key.key == Key::F1 && plain {',
        '        if key.key == Key::F1 {',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'a chord puts the list of keys away',
        '        if key.key == Key::Escape && plain && self.show_help {',
        '        if key.key == Key::Escape && self.show_help {',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'a chord works the folder view',
        '            && plain\n            && let Some(answered) = self.handle_folder_key(key)',
        '            && let Some(answered) = self.handle_folder_key(key)',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'AltGr is taken for Ctrl',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return self.handle_ctrl_chord(key);',
        '        if key.modifiers.ctrl && !key.modifiers.super_key {\n            return self.handle_ctrl_chord(key);',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'AltGr is taken for Alt',
        '        if key.modifiers.alt && !key.modifiers.ctrl && !key.modifiers.super_key {',
        '        if key.modifiers.alt && !key.modifiers.super_key {',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'a chord works the comparison',
        '        if !plain {\n            return EventResult::Ignored;\n        }\n',
        '',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        "AltGr+I toggles the search's case",
        '        if key.key == Key::I && textline::is_ctrl_chord(key.modifiers) {',
        '        if key.key == Key::I && key.modifiers.ctrl {',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        "a command's letter is typed into the search",
        '        if textline::types_into_field(key) {',
        '        if key.types_text() {',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'a chord works the search box',
        '        if !textline::is_plain(key.modifiers) {\n            return EventResult::Ignored;\n        }\n',
        '',
        ['each_kind_of_key_is_asked_for_as_itself'],
    ),
    (
        'a chord goes back to the folders',
        '        if key.key == Key::Escape && plain && self.from_folders && self.dir_compare.is_some() {',
        '        if key.key == Key::Escape && self.from_folders && self.dir_compare.is_some() {',
        ['enter_opens_the_pair_and_escape_goes_back_to_the_folders'],
    ),
    # -- the shortcut card is modal, for the keys and the pointer
    (
        "a key that is not the card's acts behind it",
        '        if self.show_help {\n'
        '            // Modal: every other key is the card\'s while it is up. It was\n',
        '        if false {\n'
        '            // Modal: every other key is the card\'s while it is up. It was\n',
        ['the_shortcut_card_takes_every_key_and_press_while_it_is_up'],
    ),
    (
        "the wheel scrolls what the card covers",
        '        if self.show_help {\n'
        '            // The card is modal for the pointer as it is for the keys: a\n',
        '        if false {\n'
        '            // The card is modal for the pointer as it is for the keys: a\n',
        ['the_shortcut_card_takes_every_key_and_press_while_it_is_up'],
    ),
    (
        "a press does not put the card away",
        '            if matches!(mouse.kind, MouseEventKind::Press(_)) {\n'
        '                self.show_help = false;\n',
        '            if matches!(mouse.kind, MouseEventKind::Press(guitk::event::MouseButton::Left)) {\n'
        '                self.show_help = false;\n',
        ['the_shortcut_card_takes_every_key_and_press_while_it_is_up'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "filediff", timeout=900, only=only))
