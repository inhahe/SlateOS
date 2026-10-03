"""Mutation test for the screen recorder's sidebar.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: the sidebar was drawn with an active row and
a hover state and answered neither -- its views were reachable only by their
keys, and `hovered_sidebar` was set by nothing.  A click opens a view now and
the row under the pointer is lit.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

SIDEBAR = "a_sidebar_row_is_opened_by_a_click_and_lit_under_the_pointer"

MUTATIONS = [
    (
        "a click on a sidebar row opens nothing",
        "                        self.active_view = view;\n                        return EventResult::Consumed;",
        "                        let _ = view;",
        [SIDEBAR],
    ),
    (
        "the row under the pointer is not lit",
        "                        self.hovered_sidebar = hovered;",
        "                        let _ = hovered;",
        [SIDEBAR],
    ),
    (
        "a row is found past the sidebar's right edge",
        "        if !(0.0..SIDEBAR_WIDTH).contains(&x) || y < SIDEBAR_NAV_TOP {",
        "        if y < SIDEBAR_NAV_TOP {",
        [SIDEBAR],
    ),
    (
        "rows are counted from the window's top, not the first row's",
        "        let row = ((y - SIDEBAR_NAV_TOP) / SIDEBAR_ITEM_H) as usize;",
        "        let row = (y / SIDEBAR_ITEM_H) as usize;",
        [SIDEBAR],
    ),
    (
        'a chord raises the list of keys',
        '        if plain && (key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift)) {',
        '        if key.key == Key::F1 || (key.key == Key::Slash && key.modifiers.shift) {',
        ['a_chord_is_not_a_recorder_key_and_altgr_is_not_ctrl'],
    ),
    (
        'a chorded Escape puts the list of keys away',
        '            if plain && matches!(key.key, Key::Escape | Key::Enter) {',
        '            if matches!(key.key, Key::Escape | Key::Enter) {',
        ['a_chord_is_not_a_recorder_key_and_altgr_is_not_ctrl'],
    ),
    (
        'AltGr is taken for Ctrl in the recorder',
        '        if textline::is_ctrl_chord(key.modifiers) {\n            return self.handle_ctrl_chord(key);',
        '        if key.modifiers.ctrl {\n            return self.handle_ctrl_chord(key);',
        ['a_chord_is_not_a_recorder_key_and_altgr_is_not_ctrl'],
    ),
    (
        "a chord works the recorder's keys",
        '        if !plain {\n            return EventResult::Ignored;\n        }\n',
        '',
        ['a_chord_is_not_a_recorder_key_and_altgr_is_not_ctrl'],
    ),
    (
        "Shift and the arrows move the history's selection, not the microphone",
        '            Key::Up | Key::Down if key.modifiers.shift => {\n                let delta = if key.key == Key::Up { 0.1 } else { -0.1 };\n                let mic',
        '            Key::Up | Key::Down if key.modifiers.shift && self.active_view != ActiveView::History => {\n                let delta = if key.key == Key::Up { 0.1 } else { -0.1 };\n                let mic',
        ['a_chord_is_not_a_recorder_key_and_altgr_is_not_ctrl'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "screenrecorder", timeout=900, only=only))
