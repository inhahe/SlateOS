"""Mutation test for the desktop-entry checks.

The crate has no code; what its tests guard is data -- each program's
`org.slateos.*.desktop`. So the mutations here break entries, one fault at a
time, and the test named for each must be the one that fails. An entry that
starts the wrong program, files its program under Other, has no picture or no
description, or offers to open files its program never reads.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

APPS = Path(__file__).resolve().parent.parent

READS = "every_program_with_a_window_has_one_entry_the_menu_can_read"
FILES = "an_entry_offers_to_open_files_only_for_a_program_that_reads_them"

TABLES = {
    "calculator/org.slateos.Calculator.desktop": [
        (
            "an entry starts a program of another name",
            "Exec=calculator\n",
            "Exec=calc\n",
            [READS],
        ),
        (
            "an entry names no main category",
            "Categories=Utility;Calculator;\n",
            "Categories=Calculator;\n",
            [READS],
        ),
        (
            "an entry has no icon",
            "Icon=accessories-calculator\n",
            "",
            [READS],
        ),
        (
            "an entry says nothing of what the program is for",
            "Comment=Do sums, from arithmetic to scientific\n",
            "",
            [READS],
        ),
        (
            "an entry offers files to a program that reads none",
            "Exec=calculator\n",
            "Exec=calculator %F\n",
            [FILES],
        ),
        (
            "an entry lists file types and takes no files",
            "Keywords=math;arithmetic;sum;\n",
            "Keywords=math;arithmetic;sum;\nMimeType=text/plain;\n",
            [FILES],
        ),
        (
            "an entry says its windowed program runs in a terminal",
            "Keywords=math;arithmetic;sum;\n",
            "Keywords=math;arithmetic;sum;\nTerminal=true\n",
            [READS],
        ),
    ],
}
# `only_programs_with_windows_have_entries` has no row: its fault is a file
# that should not exist, and the harness rewrites files that do.

if __name__ == "__main__":
    only = sys.argv[1:]
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(
            worst, sweep(APPS / file, rows, "desktopentries", timeout=300, only=mine or None)
        )
    raise SystemExit(worst)
