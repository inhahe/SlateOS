"""Mutation test for the password vault, `apps/credvault`.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Two tables:

* **the file** (`vaultfile.rs`): a header that asks for too much work is
  refused before any is done, and contents are read whole or not at all.
* **the model** (`lib.rs`): locking forgets the key and every entry, and the
  auto-lock falls due a timeout after the last use.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "vaultfile.rs"
MODEL_SRC = Path(__file__).parent / "src" / "lib.rs"

REFUSED_WHOLE = "contents_that_are_not_understood_are_refused_whole"
COSTLY = "a_file_asking_too_much_work_is_refused_before_any_is_done"
FORGETS = "locking_forgets_the_key_and_every_entry"
AUTO_LOCK = "test_vault_auto_lock"
FOLDERS = "a_folder_edit_leaves_a_vault_that_opens"
NO_SECRET = "a_debug_print_shows_no_secret"
WIPE = "wipe_overwrites_every_byte"
MADE_AT = "a_new_vault_counts_its_auto_lock_from_when_it_was_made"
HEADER_FIRST = "bytes_that_are_no_vault_are_refused_before_any_key_is_made"

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

MODEL_MUTATIONS = [
    (
        "locking keeps the entries",
        "        self.key = None;\n        self.entries.clear();\n",
        "        self.key = None;\n",
        [FORGETS],
    ),
    (
        "locking keeps the key",
        "        self.key = None;\n        self.entries.clear();\n",
        "        self.entries.clear();\n",
        [FORGETS],
    ),
    (
        "the auto-lock falls due at the last use, not a timeout after it",
        "        let due = self.last_access.saturating_add(timeout_seconds);\n",
        "        let due = self.last_access;\n",
        [AUTO_LOCK],
    ),
    (
        "an entry may be put in a folder that is not there",
        "        if folder_id.is_some_and(|f| self.get_folder(f).is_none()) {\n"
        "            return false;\n"
        "        }\n",
        "",
        [FOLDERS],
    ),
    (
        "removing a folder sends its entries to no folder, not up",
        "                entry.folder_id = removed.parent_id;\n",
        "                entry.folder_id = None;\n",
        [FOLDERS],
    ),
    (
        "removing a folder leaves the folders inside it in a folder that is gone",
        "            if folder.parent_id == Some(folder_id) {\n"
        "                folder.parent_id = removed.parent_id;\n"
        "            }\n",
        "",
        [FOLDERS],
    ),
    (
        "a login's {:?} prints its password",
        "        f.debug_struct(\"LoginData\")\n            .field(\"site\", &self.site)\n",
        "        f.debug_struct(\"LoginData\")\n            .field(\"site\", &self.site)\n"
        "            .field(\"password\", &self.password)\n",
        [NO_SECRET],
    ),
    (
        "a note's {:?} prints what it says",
        "        f.debug_struct(\"SecureNoteData\")\n            .field(\"title\", &self.title)\n",
        "        f.debug_struct(\"SecureNoteData\")\n            .field(\"title\", &self.title)\n"
        "            .field(\"content\", &self.content)\n",
        [NO_SECRET],
    ),
    (
        "wipe overwrites nothing",
        "        unsafe { core::ptr::write_volatile(byte, 0) };\n",
        "        let _ = byte;\n",
        [WIPE],
    ),
    (
        "a new vault's auto-lock counts from nought",
        "        vault.last_access = now;\n        Ok(vault)\n",
        "        Ok(vault)\n",
        [MADE_AT],
    ),
    (
        "the key is made before the header is read",
        "        let header = vaultfile::Header::parse(&self.sealed)?;\n"
        "        let mut key = vaultfile::key_for(password, header.kdf, &header.salt)?;\n",
        "        let mut key = vaultfile::key_for(password, self.kdf, &self.salt)?;\n"
        "        let _header = vaultfile::Header::parse(&self.sealed)?;\n",
        [HEADER_FIRST],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing.
    only = sys.argv[1:]
    tables = [(SRC, MUTATIONS), (MODEL_SRC, MODEL_MUTATIONS)]
    names = [name for _, rows in tables for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    results = [0]
    for src, rows in tables:
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        results.append(sweep(src, rows, "credvault", timeout=900, only=mine or None))
    raise SystemExit(max(results))
