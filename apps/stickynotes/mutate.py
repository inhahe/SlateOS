"""Mutation test for sticky notes' saving on the way out.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Ctrl+Q quit without saving, losing what had been typed since the last
autosave; the close button saved and quit whether or not the save worked.  The
table covers the repair; the rest of the suite predates it.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

FAILS = "a_close_whose_save_fails_keeps_the_window_once"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "Ctrl+Q quits without saving",
        "                Key::Q => self.quit_requested(),",
        "                Key::Q => Action::Quit,",
        ["ctrl_q_saves_first"],
    ),
    (
        "the close does not save",
        "            Event::CloseRequested => self.quit_requested(),",
        "            Event::CloseRequested => Action::Quit,",
        ["closing_the_window_saves_first"],
    ),
    (
        "a close whose save failed goes anyway",
        "        if unsaved && !self.quit_despite_failure {",
        "        if false {",
        [FAILS],
    ),
    (
        "a failed save makes the window impossible to close",
        "            self.quit_despite_failure = true;\n",
        "",
        [FAILS],
    ),
    (
        "the declined close reads to the loop as a close",
        "        if matches!(event, Event::CloseRequested) && action != Action::Quit {",
        "        if false {",
        [FAILS],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "stickynotes", timeout=900, only=only))
