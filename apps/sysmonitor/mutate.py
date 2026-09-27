"""Mutation test for the system monitor's reading of the machine's totals.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-27: `read_system`'s doc said a figure
`/proc/meminfo` does not carry keeps the value last read, and the code zeroed
it -- every field went through `unwrap_or(0)`, so a figure missing from one
read was drawn as none of that memory at all.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

READ = "the_system_figures_are_read_and_not_invented"
KEEP = "a_figure_the_file_stops_carrying_keeps_its_last_value"

MUTATIONS = [
    (
        "a figure the file does not carry is zeroed",
        "    if let Some(kib) = kib {\n        *field = kib.saturating_mul(1024);\n    }",
        "    *field = kib.unwrap_or(0).saturating_mul(1024);",
        [KEEP],
    ),
    (
        "a figure in KiB is taken for bytes",
        "        *field = kib.saturating_mul(1024);",
        "        *field = kib;",
        [READ, KEEP],
    ),
    (
        "the cache is read from the free figure",
        "            set_kib(&mut info.cached_memory, mem.cached_kib);",
        "            set_kib(&mut info.cached_memory, mem.free_kib);",
        [READ, KEEP],
    ),
    (
        "swap in use is not read",
        "            set_kib(&mut info.swap_used, mem.swap_used_kib());\n",
        "",
        [READ, KEEP],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "sysmonitor", timeout=600, only=only))
