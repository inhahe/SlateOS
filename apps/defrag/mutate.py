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
    # -- the shortcut card's hold on the pointer
    (
        'a press goes through the shortcut card',
        '            MouseEventKind::Press(_) if ui.show_help => {\n'
        '                ui.show_help = false;\n'
        '                EventResult::Consumed\n'
        '            }\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'only the left button puts the card away',
        '            MouseEventKind::Press(_) if ui.show_help => {\n',
        '            MouseEventKind::Press(MouseButton::Left) if ui.show_help => {\n',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'the wheel scrolls what the card covers',
        '            MouseEventKind::Scroll { .. } if ui.show_help => EventResult::Ignored,\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    # -- the clock, 2026-10-03: none was asked for, so a defrag never moved
    (
        'a running defrag asks for no clock',
        '        (self.defrag_state() == DefragState::Running).then_some(TICK)\n',
        '        None\n',
        ['a_running_defrag_asks_for_the_clock_and_nothing_else_does'],
    ),
    (
        'an idle window asks for the clock',
        '        (self.defrag_state() == DefragState::Running).then_some(TICK)\n',
        '        Some(TICK)\n',
        ['a_running_defrag_asks_for_the_clock_and_nothing_else_does'],
    ),
]

FIELD = 'the_exclude_box_is_the_toolkits_field'

MUTATIONS += [
    # The exclude pattern's box is the toolkit's field (2026-10-04; lane C,
    # c-e-a-theme-can-shape-the-controls).
    (
        'the box never lights',
        '            hovered: open && self.exclude_input_hovered,\n',
        '            hovered: false,\n',
        [FIELD],
    ),
    (
        'the box is never marked',
        '            focused: open && self.show_exclude_editor,\n',
        '            focused: false,\n',
        [FIELD],
    ),
    (
        'the box shows through the list of keys and the SSD question',
        '        let open = !self.show_help && !self.show_ssd_warning;\n',
        '        let open = true;\n',
        [FIELD],
    ),
    (
        'the light stays after the pointer leaves',
        '            MouseEventKind::Leave => ui.point_at(None),\n',
        '',
        [FIELD],
    ),
    (
        'a change in the light asks for no repaint',
        '        if over == self.exclude_input_hovered {\n',
        '        if true {\n',
        [FIELD],
    ),
    (
        "the focus mark is the toolkit's width, not the user's",
        '        self.focus_ring_width = settings.focus_ring_width();\n',
        '        let _ = settings;\n',
        [FIELD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "defrag", timeout=600, only=only))
