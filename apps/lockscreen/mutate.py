"""Mutation test for the lock screen.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-28: the clock's settings, `lockscreen.yaml`,
are read again when the desktop announces the file changed (C-Q26,
`design-decisions.md` §1418; the announcement is §1434).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

REREAD = "a_changed_settings_file_is_read_again_when_announced"

MUTATIONS = [
    (
        "an announced change is not read",
        "            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {",
        "            Event::SettingsChanged { group } if false && group.file_name() == CONFIG_NAME => {",
        [REREAD],
    ),
    (
        "every program's announcement is read as this screen's",
        "            Event::SettingsChanged { group } if group.file_name() == CONFIG_NAME => {",
        "            Event::SettingsChanged { group } if !group.file_name().is_empty() => {",
        [REREAD],
    ),
    (
        "a deleted file keeps the seconds it had",
        "        self.show_clock_seconds = fresh.show_clock_seconds;",
        "        self.show_clock_seconds |= fresh.show_clock_seconds;",
        [REREAD],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "lockscreen", timeout=600, only=only))
