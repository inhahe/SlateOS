"""Mutation test for the recycle bin's cross-drive move.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Covers what changed on 2026-09-27: the copy the bin falls back to across
drives followed links -- a link to a folder inside what was recycled had the
folder's contents copied into the bin.  It carries links as links now.

And, the same day, what the file manager's view of the bin needed: deleting
one entry, an id that is one name and nothing else, an Empty that says what it
could not delete, and a bin that never looks through a link -- not to list an
entry, not to describe one, not to measure one.

And on 2026-10-09, a bin on every drive with each drive's own limits
(design-decisions §1238): which bin a file goes to (`bins.rs`), which drives
there are (`drives.rs`), how a bin is held to its limits (`lib.rs`), and how
the limits are read and written (`limits.rs`).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

NO_FOLLOW = "the_cross_drive_copy_never_follows_a_link"
BAD_ID = "an_id_that_is_not_one_name_is_refused"
GONE = "deleting_an_entry_that_is_already_gone_is_not_an_error"
EMPTY_FAILS = "emptying_reports_what_it_could_not_delete"
PLANTED = "a_link_in_the_bin_folder_is_not_an_entry"
LINK_LISTED = "a_recycled_link_is_listed_as_a_link"
LINK_SIZE = "a_damaged_entry_is_measured_without_following_links"
ONE = "deleting_one_entry_leaves_the_rest"
AGE = "an_entry_past_the_age_limit_is_pruned_and_a_newer_one_kept"
OLDEST = "a_bin_over_its_number_or_size_loses_its_oldest_first"
FOLDER = "a_folder_counts_whole_against_a_size_limit"
UNDATED = "a_damaged_entry_is_not_pruned_by_any_limit"
OWN_LIMITS = "a_bins_limits_are_its_own_or_the_default"

LIB = [
    (
        "the copy follows links",
        "        let kind = entry.file_type()?;",
        "        let kind = fs::metadata(entry.path())?.file_type();",
        [NO_FOLLOW],
    ),
    (
        "any id is joined to the bin's folder",
        "    fn entry_dir(&self, id: &str) -> io::Result<PathBuf> {\n        let mut parts",
        "    fn entry_dir(&self, id: &str) -> io::Result<PathBuf> {\n"
        "        if !id.is_empty() {\n            return Ok(self.root.join(id));\n        }\n"
        "        let mut parts",
        [BAD_ID],
    ),
    (
        "an entry already gone is an error",
        "            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),\n            other => other,",
        "            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(e),\n            other => other,",
        [GONE],
    ),
    (
        "delete removes nothing",
        "        let dir = self.entry_dir(entry_id)?;\n        match fs::remove_dir_all(&dir) {",
        "        let dir = self.entry_dir(entry_id)?.join(\"nothing\");\n        match fs::remove_dir_all(&dir) {",
        [ONE],
    ),
    (
        "empty drops what it could not delete",
        "                Err(e) => outcome.failed.push((entry, e)),",
        "                Err(_) => {}",
        [EMPTY_FAILS],
    ),
    (
        "the listing looks through a link",
        "            if !dir_entry.file_type().is_ok_and(|t| t.is_dir()) {",
        "            if !dir_entry.path().is_dir() {",
        [PLANTED],
    ),
    (
        "a recycled link is described as what it names",
        "        let (size, is_dir, is_link) = match fs::symlink_metadata(&data_path) {",
        "        let (size, is_dir, is_link) = match fs::metadata(&data_path) {",
        [LINK_LISTED],
    ),
    (
        "a damaged entry is measured through its links",
        "                    Ok(kind) if kind.is_dir() => pending.push(child.path()),",
        "                    Ok(_) if child.path().is_dir() => pending.push(child.path()),",
        [LINK_SIZE],
    ),
    # -- holding a bin to its limits --
    (
        "the age limit takes nothing",
        "            let too_old = limits.max_age.is_some_and(|max| age > max);",
        "            let too_old = false && limits.max_age.is_some_and(|max| age > max);",
        [AGE],
    ),
    (
        "the number limit takes nothing",
        "            let too_many = limits.max_items.is_some_and(|max| items > u64::from(max));",
        "            let too_many = false && limits.max_items.is_some_and(|max| items > u64::from(max));",
        [OLDEST],
    ),
    (
        "the size limit takes nothing",
        "            let too_big = limits.max_bytes.is_some_and(|max| bytes > max);",
        "            let too_big = false && limits.max_bytes.is_some_and(|max| bytes > max);",
        [OLDEST],
    ),
    (
        "the newest go first",
        "        let mut items = u64::try_from(entries.len()).unwrap_or(u64::MAX);",
        "        order.reverse();\n        let mut items = u64::try_from(entries.len()).unwrap_or(u64::MAX);",
        [OLDEST],
    ),
    (
        "an entry deleted still counts against the limits",
        "                    items = items.saturating_sub(1);\n",
        "",
        [OLDEST],
    ),
    (
        "a folder counts as its entry's size, nothing",
        "                Self::entry_size(&self.root.join(&entry.id))\n            } else {",
        "                entry.size\n            } else {",
        [FOLDER],
    ),
    (
        "an entry of unknown age is taken by a limit",
        "            let Some(recycled_at) = entry.recycled_at else {\n"
        "                // Undated, and every later one is too.\n                break;\n            };",
        "            let recycled_at = entry.recycled_at.unwrap_or(SystemTime::UNIX_EPOCH);",
        [UNDATED],
    ),
    (
        "a bin's own limits are not read",
        "            Ok(Some(own)) => own,",
        "            Ok(Some(_)) => *default,",
        [OWN_LIMITS],
    ),
    (
        "limits that cannot be read are taken as the default",
        "            Err(_) => Limits::NONE,",
        "            Err(_) => *default,",
        [OWN_LIMITS],
    ),
    (
        "a bin without limits of its own is said to have them",
        "            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),",
        "            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Some(Limits::NONE)),",
        [OWN_LIMITS],
    ),
    (
        "limits taken away stay",
        "            return match fs::remove_file(&path) {",
        "            return match Ok::<(), io::Error>(()) {",
        [OWN_LIMITS],
    ),
    (
        "the usage counts nothing",
        "            usage.items = usage.items.saturating_add(1);\n",
        "",
        [FOLDER],
    ),
]

# -- which bin a file goes to --
BINS = [
    (
        "a file on another drive goes to the home bin",
        "            (Some(drive), Some(home)) if drive != *home => DriveBin {",
        "            (Some(drive), Some(home)) if drive != *home && false => DriveBin {",
        ["a_file_goes_to_the_bin_on_its_own_drive"],
    ),
    (
        "with the home drive unknown, a file goes to its drive's bin",
        "            (Some(drive), Some(home)) if drive != *home => DriveBin {",
        "            (Some(drive), _) if Some(&drive) != self.home_drive.as_ref() => DriveBin {",
        ["when_drives_cannot_be_told_apart_every_file_goes_home"],
    ),
    (
        "a drive's bin that is not a folder is used",
        "        meta.is_dir()\n            && !meta.file_type().is_symlink()",
        "        true\n            && !meta.file_type().is_symlink()",
        ["a_bin_that_is_not_a_folder_is_not_used"],
    ),
    (
        "a drive with no bin is listed",
        "            if fs::symlink_metadata(&root).is_ok_and(|meta| self.is_ours(&meta)) {",
        "            if true {",
        ["a_drive_with_nothing_deleted_has_no_bin_listed"],
    ),
    (
        "every bin is held to the default",
        "                let limits = drive.bin.limits(default);",
        "                let limits = *default;",
        ["each_bin_is_held_to_its_own_limits"],
    ),
]

# -- which drives there are --
DRIVES = [
    (
        "a view of the kernel is taken for a drive",
        "        if NOT_DRIVES.iter().any(|n| n.as_bytes() == kind.as_slice()) {",
        "        if false && NOT_DRIVES.iter().any(|n| n.as_bytes() == kind.as_slice()) {",
        ["the_drives_are_read_from_the_mount_table"],
    ),
    (
        "a mount point's escapes are not read",
        "        let point = unescape_octal(point);",
        "        let point = point.to_vec();",
        ["the_drives_are_read_from_the_mount_table"],
    ),
    (
        "a relative mount point is taken",
        "        if point.first() != Some(&b'/') {\n            continue;\n        }\n",
        "",
        ["the_drives_are_read_from_the_mount_table"],
    ),
    (
        "an escape past a byte is read",
        "                .and_then(|text| u8::from_str_radix(text, 8).ok())",
        "                .and_then(|text| u32::from_str_radix(text, 8).ok().map(|v| v as u8))",
        ["an_escape_is_read_as_its_byte_and_nothing_else_is"],
    ),
    (
        "a drive is found by the letters of its name",
        "        .filter(|m| path.starts_with(m))",
        "        .filter(|m| path.to_string_lossy().starts_with(&*m.to_string_lossy()))",
        ["a_file_is_on_the_drive_mounted_nearest_it"],
    ),
    (
        "the farthest drive is the one a file is on",
        "        .max_by_key(|m| m.components().count())",
        "        .min_by_key(|m| m.components().count())",
        ["a_file_is_on_the_drive_mounted_nearest_it"],
    ),
]

# -- the limits, read and written --
LIMITS = [
    (
        "a limit of zero deletes everything",
        "                .filter(|n| *n > 0)\n",
        "",
        ["a_limit_that_cannot_be_read_is_no_limit"],
    ),
    (
        "an age is written rounded down",
        "            self.max_age.map(|age| age.as_secs().div_ceil(DAY_SECS)),",
        "            self.max_age.map(|age| age.as_secs() / DAY_SECS),",
        ["a_limit_is_written_whole_and_never_tighter"],
    ),
    (
        "a limit taken away stays in the file",
        "                    let _was_there = doc.remove(&at);",
        "                    let _was_there = false;",
        ["limits_read_back_as_written"],
    ),
    (
        "keeping everything reads back as thirty days",
        '            doc.set_bool(&[USER_SECTION, "keep_everything"], true);',
        "",
        ["the_users_default_is_thirty_days_until_they_set_one"],
    ),
    (
        "the user's default is never read",
        "        if doc.keys(&[USER_SECTION]).is_empty() {",
        "        if true {",
        ["the_users_default_is_thirty_days_until_they_set_one"],
    ),
]

TABLES = {
    "lib.rs": LIB,
    "bins.rs": BINS,
    "drives.rs": DRIVES,
    "limits.rs": LIMITS,
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
        worst = max(worst, sweep(SRC / file, rows, "recyclebin", timeout=900, only=mine))
    raise SystemExit(worst)
