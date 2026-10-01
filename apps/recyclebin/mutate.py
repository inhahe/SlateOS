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

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "lib.rs"

NO_FOLLOW = "the_cross_drive_copy_never_follows_a_link"
BAD_ID = "an_id_that_is_not_one_name_is_refused"
GONE = "deleting_an_entry_that_is_already_gone_is_not_an_error"
EMPTY_FAILS = "emptying_reports_what_it_could_not_delete"
PLANTED = "a_link_in_the_bin_folder_is_not_an_entry"
LINK_LISTED = "a_recycled_link_is_listed_as_a_link"
LINK_SIZE = "a_damaged_entry_is_measured_without_following_links"
ONE = "deleting_one_entry_leaves_the_rest"

MUTATIONS = [
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
]

if __name__ == "__main__":
    sys.exit(sweep(SRC, MUTATIONS, "recyclebin", timeout=900))
