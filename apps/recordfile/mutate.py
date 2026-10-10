"""Mutation test for the shared data file's merge, ids and watch.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what two windows of one program need to save into one file without
losing each other's work (design-decisions §1239): ids no other window will
give out, a three-way merge by id that keeps every window's changes, and the
watch's reading of the folder's events.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

ADDED = "what_two_windows_added_is_all_kept"
EDITS = "edits_to_different_records_both_stand_and_to_one_the_later_wins"
DELETES = "deletions_stand_but_not_over_a_later_edit"
ORDER = "order_is_a_change_too"
IDS = "a_fresh_id_is_in_range_and_not_taken"
WATCH = "the_watch_tells_only_of_the_file_and_waits_for_an_in_place_write"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    (
        "a record changed here is not taken",
        "            Some(mine) if changed_here(&k) => result.push(mine.clone()),\n",
        "            Some(_) if changed_here(&k) => result.push(record.clone()),\n",
        [EDITS],
    ),
    (
        "a record deleted here stays",
        "        if deleted_here(&k) {\n            continue;\n        }\n",
        "",
        [DELETES],
    ),
    (
        "a record added here is not added",
        "        result.insert(index.min(result.len()), record.clone());\n",
        "        let _ = index;\n",
        [ADDED],
    ),
    (
        "an edit here loses to a deletion there",
        "        if placed.contains(&k) || !changed_here(&k) {\n",
        "        if placed.contains(&k) || !changed_here(&k) || base_by.contains_key(&k) {\n",
        [DELETES],
    ),
    (
        "a reorder here is not kept",
        "    let reordered = shared(base) != shared(mine);\n",
        "    let reordered = false && shared(base) != shared(mine);\n",
        [ORDER],
    ),
    (
        "what this window adds goes before what another added first",
        "            !base_by.contains_key(&k) && !mine_by.contains_key(&k)\n        }) {\n",
        "            false && !base_by.contains_key(&k) && !mine_by.contains_key(&k)\n        }) {\n",
        [ADDED],
    ),
    (
        "a fresh id may be one in use",
        "        if last != 0 && !taken(last) {\n",
        "        if last != 0 {\n",
        [IDS],
    ),
    (
        "a fresh id is past what a JSON number holds",
        "        last = hasher.finish() & MAX_ID;\n",
        "        last = hasher.finish() | (1 << 60);\n",
        [IDS],
    ),
    (
        "events lost are not told",
        "        return Verdict::Now;\n    }\n",
        "        return Verdict::Nothing;\n    }\n",
        [WATCH],
    ),
    (
        "another file's events are told",
        "    if !ours(name) {\n        return Verdict::Nothing;\n    }\n",
        "",
        [WATCH, "a_folder_watch_tells_of_every_file_of_its_kind"],
    ),
    (
        "an in-place write is told before it is finished",
        "        Verdict::WhenQuiet\n    } else {",
        "        Verdict::Now\n    } else {",
        [WATCH],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "recordfile", timeout=300, only=only))
