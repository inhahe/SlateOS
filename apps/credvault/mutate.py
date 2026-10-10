"""Mutation test for the vault's file format, `apps/credvault`.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The file itself: a header that asks for too much work is refused before any
is done, and contents are read whole or not at all.  Moved here from
`apps/credmanager/mutate.py` with the format, 2026-10-10.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "vaultfile.rs"

REFUSED_WHOLE = "contents_that_are_not_understood_are_refused_whole"
COSTLY = "a_file_asking_too_much_work_is_refused_before_any_is_done"

MUTATIONS = [
    (
        "a file asking any amount of memory is opened",
        "        if kdf.memory_kib > MAX_MEMORY_KIB\n",
        "        if false\n",
        [COSTLY],
    ),
    (
        "a record not understood is skipped",
        '            _ => return Err(bad("a record this program does not know")),',
        "            _ => {}",
        [REFUSED_WHOLE],
    ),
    (
        "an escape never written is read anyway",
        '            textfmt::tsv::unescape(raw).ok_or_else(|| bad("a field is not escaped as written"))',
        "            Ok((*raw).to_string())",
        [REFUSED_WHOLE],
    ),
    (
        "the next id may be one in use",
        "    if contents.next_id <= highest {",
        "    if false {",
        [REFUSED_WHOLE],
    ),
    (
        "an entry may be in a folder that is not there",
        "            if let Some(f) = folder_id\n                && !c.folders.iter().any(|folder| folder.id == f)\n",
        "            if let Some(f) = folder_id\n                && false\n",
        [REFUSED_WHOLE],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "credvault", timeout=900, only=only))
