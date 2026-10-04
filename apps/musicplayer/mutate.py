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
    # -- the shortcut card's hold on the pointer
    (
        'a press goes through the shortcut card',
        '            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                state.show_help = false;\n'
        '                return true;\n'
        '            }\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'only the left button puts the card away',
        '            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n',
        '            MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'the wheel scrolls what the card covers',
        '            MouseEventKind::Scroll { .. } => return false,\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
]

FIELD = "the_search_box_is_the_toolkits_field"
CARET = "the_search_caret_is_a_caret_after_the_query"

MUTATIONS += [
    # The search box is the toolkit's field (2026-10-04; lane C,
    # c-e-a-theme-can-shape-the-controls).
    (
        "the search box never has the keyboard",
        "        focused: state.searching && !state.show_help,\n",
        "        focused: false,\n",
        [FIELD, CARET],
    ),
    (
        "the search box keeps its mark under the card",
        "        focused: state.searching && !state.show_help,\n",
        "        focused: state.searching,\n",
        [FIELD],
    ),
    (
        "a query that finds nothing is not red",
        "        invalid: !state.search_query.is_empty() && state.filtered_library().is_empty(),\n",
        "        invalid: false,\n",
        [FIELD],
    ),
    (
        "the search box is drawn with no search open",
        "    if state.searching {\n        let search = search_box_rect(state.width);\n",
        "    if true {\n        let search = search_box_rect(state.width);\n",
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        "        self.focus_ring_width = settings.focus_ring_width();\n",
        "        let _ = settings;\n",
        [FIELD],
    ),
    (
        "the caret is at the start of the query",
        "                cursor: guitk::text::TextCursor::from(state.search_query.len()),\n",
        "                cursor: guitk::text::TextCursor::from(0),\n",
        [CARET],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "musicplayer", timeout=600, only=only))
