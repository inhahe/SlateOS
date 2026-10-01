"""Mutation test for snapstore, the store the backup tool and System Restore share.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers the promises in the crate's docs and the faults the move out
of `apps/backup` (2026-09-27) found: an id reused within a second, a blob
named by a stale hash, a file that could not be read left out silently, an
incremental that brought deleted files back, a parent from another folder, a
collection that deleted what an unreadable record named, a restore that wrote
through a link, and a month that was thirty days.

Two promises are pinned by tests that run only on Unix -- a restore does not
write through a link, and a private file comes back private -- because Windows
needs privilege to make a link and has no permission bits to restore. They
are not rows here: the harness runs on the host, where those tests do not
exist. Run `cargo test -p snapstore` under Linux for them.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

IDS = "a_capture_never_reuses_a_taken_id"
INGEST = "a_blob_is_named_by_what_was_stored"
LOCKED = "a_mirror_restore_leaves_a_file_that_was_locked"
EXCLUDED = "a_mirror_restore_leaves_what_was_excluded"
LEGACY_DIRS = "a_legacy_snapshot_removes_no_folder"
MIRROR = "a_mirror_restore_makes_the_folder_match"
SAME_SIZE = "a_same_size_change_is_restored"
PARENT = "an_incremental_takes_its_parent_from_the_same_folder"
GC_REFUSE = "collection_refuses_while_a_record_is_unreadable"
GC_GRACE = "collection_leaves_young_orphans_and_removes_old_ones"
ID_REFUSED = "an_id_that_leaves_the_store_is_refused"
LEGACY = "a_legacy_incremental_is_rebuilt_from_its_chain"
DELETED = "a_file_deleted_before_an_incremental_stays_deleted"
MODE = "a_mode_survives_the_round_trip"
MODE_WHOLE = "a_mode_that_is_not_whole_is_refused"
WHOLE = "only_a_whole_number_is_a_u64"
MONTHS = "keep_monthly_counts_calendar_months"
STANDALONE = "a_standalone_snapshot_does_not_hold_its_parent"

LIB = [
    (
        "a taken id is reused",
        "            match fs::create_dir(&dir) {",
        "            match fs::create_dir_all(&dir) {",
        [IDS],
    ),
    (
        "what could not be read is not recorded",
        "        manifest.unread.clone_from(&unread);\n",
        "",
        [LOCKED],
    ),
    (
        "a mirror restore removes what it never read",
        "protected.iter().any(|p| rel.starts_with(p)) || is_excluded(rel, &manifest.excluded)",
        "is_excluded(rel, &manifest.excluded)",
        [LOCKED],
    ),
    (
        "a mirror restore removes what was excluded",
        "protected.iter().any(|p| rel.starts_with(p)) || is_excluded(rel, &manifest.excluded)",
        "protected.iter().any(|p| rel.starts_with(p))",
        [EXCLUDED],
    ),
    (
        "a snapshot that never recorded folders removes them",
        "        if manifest.dirs_recorded {",
        "        if true {",
        [LEGACY_DIRS],
    ),
    (
        "a snapshot that recorded folders removes none",
        "        if manifest.dirs_recorded {",
        "        if false {",
        [MIRROR],
    ),
    (
        "a file of the same size is taken for unchanged",
        "                && sha256_file(&target).is_ok_and(|h| h == entry.hash)\n",
        "\n",
        [SAME_SIZE],
    ),
    (
        "a parent is taken from any folder",
        "m.source == source && ",
        "",
        [PARENT],
    ),
    (
        "collection runs past an unreadable record",
        "        if let Some((dir, why)) = listing.unreadable.first() {",
        "        if let Some((dir, why)) = None::<&(PathBuf, String)> {",
        [GC_REFUSE],
    ),
    (
        "an unreadable record is not listed as one",
        "                    Err(e) => listing.unreadable.push((name, e)),",
        "                    Err(_) => {}",
        [GC_REFUSE],
    ),
    (
        "collection ignores its grace period",
        "            if !referenced.contains(&hash) && old_enough(written) {",
        "            if !referenced.contains(&hash) {",
        [GC_GRACE],
    ),
    (
        "an id may hold a path",
        "        && !id.bytes().any(|b| b == b'/' || b == b'\\\\' || b == 0)",
        "",
        [ID_REFUSED],
    ),
    (
        "a legacy chain is applied oldest-last",
        "        for m in chain.iter().rev() {",
        "        for m in chain.iter() {",
        [LEGACY],
    ),
    (
        "an incomplete snapshot may be mirrored",
        "        if opts.mirror && !manifest.complete {",
        "        if false {",
        [LEGACY],
    ),
    (
        "an unchanged file is left out of an incremental",
        "            let entry = if let Some(prev) = reused {\n"
        "                dedup_blobs = dedup_blobs.saturating_add(1);\n"
        "                prev.clone()\n",
        "            let entry = if let Some(_prev) = reused {\n"
        "                dedup_blobs = dedup_blobs.saturating_add(1);\n"
        "                progress(&p);\n"
        "                continue;\n",
        [DELETED],
    ),
]

STORE = [
    (
        "a blob is taken as stored without being there",
        "        if !expected.is_empty() && self.has_blob(expected) {",
        "        if !expected.is_empty() {",
        [INGEST],
    ),
]

MANIFEST = [
    (
        "the mode is not written",
        "        if let Some(mode) = mode {\n",
        "        if let Some(mode) = mode.as_ref().filter(|_| false) {\n",
        [MODE],
    ),
]

JSON = [
    (
        "a fraction reads as a whole number",
        "n.is_finite() && *n >= 0.0 && n.trunc() == *n && *n <= EXACT",
        "n.is_finite() && *n >= 0.0 && *n <= EXACT",
        [WHOLE, MODE_WHOLE],
    ),
]

RETENTION = [
    (
        "a month is thirty days again",
        "    per_period(policy.keep_monthly, &month_of);",
        "    per_period(policy.keep_monthly, &|t| {\n"
        "        i64::try_from(t / (86_400 * 30)).unwrap_or(i64::MAX)\n"
        "    });",
        [MONTHS],
    ),
    (
        "a standalone snapshot holds its chain",
        "        if standalone.contains(&id) {\n"
        "            // Every file is in its own manifest: it needs nothing else.\n"
        "            continue;\n"
        "        }\n",
        "",
        [STANDALONE],
    ),
]

TABLES = {
    "lib.rs": LIB,
    "store.rs": STORE,
    "manifest.rs": MANIFEST,
    "json.rs": JSON,
    "retention.rs": RETENTION,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "snapstore", timeout=900, only=mine or None))
    raise SystemExit(worst)
