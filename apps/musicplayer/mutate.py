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
    # No row for "a command's letter is typed into the search": since
    # 2026-10-04 the search box's typing is textline::apply_key's, which tells
    # a command from AltGr itself, in its own crate and with its own tests;
    # f1_raises_the_keys_and_a_chord_is_neither_a_player_key_nor_typing still
    # holds the box to it.
    (
        'a chord works the search box',
        '        if plain {\n            match key_event.key {\n                Key::Escape => {\n',
        '        if true {\n            match key_event.key {\n                Key::Escape => {\n',
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
        "    if state.search_box_shown() && search.w > 0.0 {\n",
        "    if search.w > 0.0 {\n",
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
        "                cursor: state.search_cursor(),\n",
        "                cursor: TextCursor::default(),\n",
        [CARET],
    ),
]

# The search box edits at a caret (2026-10-04,
# known-issues/E-twenty-nine-applications-type-only-at-the-end-of-a-box): it
# took typing at its end and Backspace from it, and nothing else. And Enter
# keeps the search on screen: it took the box away and left the library
# filtered by a query nothing showed.
EDITS = "the_search_box_edits_at_a_caret"
ENTER = "enter_keeps_the_search_on_screen"
PRESS = "a_press_outside_the_search_box_gives_the_keyboard_back"
TABS_ = "the_search_box_covers_no_tab"
SHOWN = "the_box_edits_the_search_it_shows"

MUTATIONS += [
    (
        "Enter takes the box away with the search still on",
        "        self.searching || !self.search_query.is_empty()\n",
        "        self.searching\n",
        [ENTER],
    ),
    (
        "Ctrl+F does not select what the box holds",
        "        self.search_editor.select_all();\n",
        "",
        [ENTER],
    ),
    (
        "a cut or a copy takes nothing to the clipboard",
        "            self.search_clipboard = copied;\n",
        "            let _ = copied;\n",
        [EDITS],
    ),
    (
        "a key finds the editor holding another search",
        "    fn search_key(&mut self, key: &KeyEvent) -> bool {\n        if self.search_editor.text() != self.search_query {\n",
        "    fn search_key(&mut self, key: &KeyEvent) -> bool {\n        if false {\n",
        [SHOWN],
    ),
    (
        "a press finds the editor holding another search",
        "        self.searching = true;\n        if self.search_editor.text() != self.search_query {\n",
        "        self.searching = true;\n        if false {\n",
        [SHOWN],
    ),
    (
        "a press puts the caret at the start",
        "            x - rect.x - SEARCH_TEXT_INSET,\n",
        "            0.0,\n",
        [EDITS],
    ),
    (
        "a press in the box does not give it the keyboard",
        "                state.press_search(x);\n",
        "",
        [EDITS, PRESS],
    ),
    (
        "a press elsewhere leaves the box the keyboard",
        "            let blurred = std::mem::replace(&mut state.searching, false);\n",
        "            let blurred = false;\n",
        [PRESS],
    ),
    (
        "a press on nothing does not redraw the box it took the keyboard from",
        "            blurred\n        }\n",
        "            false\n        }\n",
        [PRESS],
    ),
    (
        "a window chord is lost to an open search",
        "        if !textline::is_ctrl_chord(key_event.modifiers) {\n            return false;\n        }\n    }\n",
        "        return false;\n    }\n",
        [PRESS],
    ),
    (
        "a plain key the box does not answer works the player",
        "        if !textline::is_ctrl_chord(key_event.modifiers) {\n            return false;\n        }\n    }\n",
        "    }\n",
        [ENTER],
    ),
    (
        "Escape leaves a search the box has no keyboard for",
        "        Key::Escape if !state.search_query.is_empty() => {\n",
        "        Key::Escape if false => {\n",
        [ENTER],
    ),
    (
        "the selection is not drawn",
        "                selection_anchor: if editing {\n",
        "                selection_anchor: if false {\n",
        [EDITS],
    ),
    (
        "the box covers the tabs in a narrow window",
        "    let left = (right - 240.0).max(tab_left(TABS.len()));\n",
        "    let left = right - 240.0;\n",
        [TABS_],
    ),
    (
        "a box with no room is drawn",
        "    if state.search_box_shown() && search.w > 0.0 {\n",
        "    if state.search_box_shown() {\n",
        [TABS_],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "musicplayer", timeout=600, only=only))
