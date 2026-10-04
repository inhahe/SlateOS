"""Mutation test for stopwatch.

Its keys are its own only with nothing but Shift held: a chord with Ctrl, Alt
or the Windows key is the window's or the desktop's.  And, since 2026-10-04,
the list of keys takes the pointer: a press with it up puts it away and
reaches nothing under it, and the wheel scrolls nothing it covers.

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
        "a key held with a modifier is the stopwatch's",
        '        Event::Key(key) if key.pressed && !textline::is_plain(key.modifiers) => {\n            EventResult::Ignored\n        }\n',
        '',
        ['a_key_held_with_a_modifier_is_not_the_stopwatchs'],
    ),
]

CARD = 'the_shortcut_card_takes_a_press_rather_than_passing_it_on'

MUTATIONS += [
    # The list of keys takes the pointer (2026-10-04; known-issues
    # E-a-press-goes-through-the-shortcut-card-to-the-control-drawn-under-it).
    (
        'a press goes through the list of keys',
        '    if state.show_help {\n'
        '        return match mouse.kind {\n'
        '            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                state.show_help = false;\n'
        '                EventResult::Consumed\n'
        '            }\n'
        '            _ => EventResult::Ignored,\n'
        '        };\n'
        '    }\n',
        '',
        [CARD],
    ),
    (
        'only the left button puts the list of keys away',
        '            MouseEventKind::Press(_) | MouseEventKind::DoubleClick(_) => {\n'
        '                state.show_help = false;\n',
        '            MouseEventKind::Press(MouseButton::Left) | MouseEventKind::DoubleClick(_) => {\n'
        '                state.show_help = false;\n',
        [CARD],
    ),
    (
        'a press under the list of keys leaves it up',
        '                state.show_help = false;\n'
        '                EventResult::Consumed\n',
        '                EventResult::Consumed\n',
        [CARD],
    ),
    (
        'the wheel scrolls what the list of keys covers',
        '    if state.show_help {\n'
        '        return match mouse.kind {\n',
        '    if state.show_help && !matches!(mouse.kind, MouseEventKind::Scroll { .. }) {\n'
        '        return match mouse.kind {\n',
        [CARD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "stopwatch", timeout=600, only=only))
