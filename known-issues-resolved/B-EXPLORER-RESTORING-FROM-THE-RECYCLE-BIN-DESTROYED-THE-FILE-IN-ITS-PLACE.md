## B-EXPLORER-RESTORING-FROM-THE-RECYCLE-BIN-DESTROYED-THE-FILE-IN-ITS-PLACE (lane C, 2026-08-16) — FIXED

**In short:** you delete `notes.txt`, then later make a *new* `notes.txt` in the
same folder, then change your mind and restore the old one from the recycle
bin. The old file landed on top of the new one and the new one was gone — not
to the recycle bin, just gone. The recycle bin is the feature whose entire
promise is "deleting is undoable"; it was the thing doing the undoable
deleting. Fixed: a restore that finds something already at the original path
now lands beside it as `notes (2).txt`, and reports where it actually put the
file.

### The bug

`RecycleBin::restore` in `apps/explorer/src/fileops.rs` moved the recycled data
straight to the path recorded at delete time:

```rust
pub fn restore(&self, entry_id: &str) -> io::Result<()> {
    let entry = self.read_entry(entry_id)?;
    let data_path = self.root.join(entry_id).join("data");
    if let Some(parent) = entry.original_path.parent() {
        fs::create_dir_all(parent)?;
    }
    move_path(&data_path, &entry.original_path)?;   // <-- unconditional
    …
}
```

Nothing checked whether `original_path` was still free. `move_path` overwrites,
so whatever occupied the path was destroyed in place.

The rest of the explorer already knew better — `resolve_rename` (same file) is
exactly the "pick a free `name (2)` variant" helper the copy/move engine uses on
a name collision, and had been for as long as the engine has existed. The
recycle bin simply never called it. This is the same shape as
`B-EXPLORER-RENAME-OVERWROTE-AND-ESCAPED` above: the conflict policy existed and
one code path did not consult it.

Two aggravating details:

- **The window is unbounded.** A copy/move collision happens while the user is
  watching a progress dialog. A restore collision happens an arbitrary time
  after the delete, against a file the user made in between and has no reason to
  associate with the recycle bin at all.
- **The destroyed file did not go to the recycle bin either.** There was nothing
  left to recover from anywhere.

### The fix

`restore` now returns `io::Result<PathBuf>` — **where the file actually landed**,
which is what the caller must show the user:

```rust
let dest = if entry.original_path.exists() {
    resolve_rename(&entry.original_path)
} else {
    entry.original_path.clone()
};
move_path(&data_path, &dest)?;
```

Returning the path rather than silently relocating matters: a restore that puts
your file somewhere other than where you asked, and does not say so, is only
marginally better than one that overwrites.

The gap between the `exists()` check and the move is a genuine race that `std`
alone cannot close — there is no "rename only if the destination is free" in the
standard library. The same tradeoff is already documented on the engine's
conflict handling; narrowing it needs a platform primitive we do not have yet.

While in there, the entry-directory cleanup after a successful restore stopped
discarding its errors. A `meta.txt` that outlives its `data` leaves an entry
`list` still shows and `restore` can never satisfy again, and the user's only
clue would be the failure of some restore attempted much later.

### Verification

Three tests in `apps/explorer/src/fileops.rs`:
`restoring_over_a_newer_file_of_the_same_name_keeps_both`,
`restoring_to_a_free_path_uses_the_original_name`, and
`restoring_a_directory_does_not_merge_into_one_that_is_in_the_way`.

Re-introducing the bug (`let dest = entry.original_path.clone();`) turned
exactly two of them red — the newer-file one and the directory one — and only
those, out of 207 (205 passed / 2 failed). Green again on revert.
`restoring_to_a_free_path_uses_the_original_name` passes *with* the bug, as it
must: on a free path the two implementations are identical. That is the
happy-path blindness that let this survive.

Clippy: explorer 46 non-test warnings before, 46 after.
