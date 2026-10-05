"""Mutation test for benchmark.

Its keys: Ctrl+E and Ctrl+Q are Ctrl chords, not AltGr, and every other binding
is its own only with nothing but Shift held.

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
        "a key held with a modifier is the window's",
        '        if !export && !textline::is_plain(key.modifiers) {',
        '        if false {',
        ['only_the_windows_own_chords_are_taken_and_altgr_is_not_one'],
    ),
    (
        'AltGr+E exports',
        '        let export = key.key == Key::E && textline::is_ctrl_chord(key.modifiers);',
        '        let export = key.key == Key::E && key.modifiers.ctrl;',
        ['only_the_windows_own_chords_are_taken_and_altgr_is_not_one'],
    ),
    (
        'AltGr+Q closes the window',
        '            && textline::is_ctrl_chord(key.modifiers)',
        '            && key.modifiers.ctrl',
        ['only_the_windows_own_chords_are_taken_and_altgr_is_not_one'],
    ),
    # -- the shortcut card's hold on the pointer
    (
        'a press goes through the shortcut card',
        '                    MouseEventKind::Press(_) if self.show_help => {\n'
        '                        self.show_help = false;\n'
        '                        EventResult::Consumed\n'
        '                    }\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'only the left button puts the card away',
        '                    MouseEventKind::Press(_) if self.show_help => {\n',
        '                    MouseEventKind::Press(MouseButton::Left) if self.show_help => {\n',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    (
        'the wheel scrolls what the card covers',
        '                    MouseEventKind::Scroll { .. } if self.show_help => EventResult::Ignored,\n',
        '',
        ['the_shortcut_card_takes_a_press_rather_than_passing_it_on'],
    ),
    # -- the clock, 2026-10-03: none was asked for
    (
        'a running suite asks for no clock',
        '        self.progress.phase.is_running().then_some(TICK)\n',
        '        None\n',
        ['a_running_suite_asks_for_the_clock_and_an_idle_one_does_not'],
    ),
    (
        'an idle window asks for the clock',
        '        self.progress.phase.is_running().then_some(TICK)\n',
        '        Some(TICK)\n',
        ['a_running_suite_asks_for_the_clock_and_an_idle_one_does_not'],
    ),
    (
        'a tick with nothing running asks for a repaint',
        '                    EventResult::Consumed\n'
        '                } else {\n'
        '                    EventResult::Ignored\n'
        '                }\n'
        '            }\n'
        '            // Through `resize`',
        '                    EventResult::Consumed\n'
        '                } else {\n'
        '                    EventResult::Consumed\n'
        '                }\n'
        '            }\n'
        '            // Through `resize`',
        ['a_running_suite_asks_for_the_clock_and_an_idle_one_does_not'],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "benchmark", timeout=600, only=only))
