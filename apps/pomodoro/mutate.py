"""Mutation test for the pomodoro timer's settings.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: "Notification Sound: On" was a row the
arrows flipped and nothing else read -- a session ends in silence either way,
because nothing here plays a sound.  The settings say so beneath their rows
now, and the rows leave room for it at any height they fit in.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

SOUND = "the_sound_row_says_nothing_plays_it"

MUTATIONS = [
    (
        "the note is not drawn",
        "        if below + 12.0 <= layout.content.bottom() {",
        "        if false {",
        [SOUND],
    ),
    (
        "the rows leave no room for the note",
        "        ((self.content.h - 80.0) / rows).clamp(18.0, 34.0)",
        "        ((self.content.h - 60.0) / rows).clamp(18.0, 34.0)",
        [SOUND],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "pomodoro", timeout=900, only=only))
