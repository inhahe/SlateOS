"""Mutation test for stopwatch.

Its keys are its own only with nothing but Shift held: a chord with Ctrl, Alt
or the Windows key is the window's or the desktop's.

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

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "stopwatch", timeout=600, only=only))
