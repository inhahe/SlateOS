## TD-C-RENAMER-SAID-RENAMED-N-FILES-AND-RENAMED-NOTHING -- FIXED 2026-09-15

**In short:** the bulk renamer opened showing six files that did not exist,
let you build up rename rules against them, and when you pressed Rename it
said "Renamed 6 files" and changed nothing on the disk. Undo said "Undid
rename of 6 files" and also changed nothing. The program had no filesystem
access of any kind. It now opens a real folder, renames real files, and says
what actually happened to each.

**Date:** 2026-09-15. **Lane:** C.

**This is the `undelete`/`partmanager`/`netscan` family**, recorded under
`TD-C-THE-THREE-TOOLS-THAT-REPORT-ACTS-THEY-DID-NOT-PERFORM`, and it was missed
by that sweep for a reason worth keeping: **that sweep looked for fabricated
readings, and the renamer's fabrication was its input list while its false
claim was its status line.** A scanner looking for invented *data* finds a
program that shows you numbers it made up. It does not find one whose invented
data is a plausible list of filenames and whose lie is a past-tense verb.

What believing it costs is the same as the other three, and in the same
direction: a person told a batch rename **failed** looks for their files under
the old names. One told it **succeeded** looks under the new ones, finds
nothing, and may well conclude the files are lost -- or delete the copy they
kept, the rename having "worked".

| control | claimed | did |
|---|---|---|
| Enter (Rename) | "Renamed 6 files" | edited `Vec<FileEntry>` |
| Ctrl+Z (Undo) | "Undid rename of 6 files" | edited `Vec<FileEntry>` |
| Ctrl+Y (Redo) | "Redid rename of 6 files" | edited `Vec<FileEntry>` |
| the file list | six files under `/home/user/...` | invented at startup by `seed_sample_files` |

**The undo is the dangerous one and it nearly survived the fix.** Repairing
`execute_rename` alone would have left the identical defect in `undo` and
`redo`, which both called the same memory-only `apply_plan` -- and an undo that
reports success while restoring nothing is worse than a rename that does
nothing, because by then the files really have moved and the user has been told
they are back.

**What the fix did NOT have to build.** `rename_plan` was already there,
already correct, and already tested: it orders a batch so no step overwrites a
name a later step still needs, and parks a name under a temporary when a cycle
makes that impossible. It was written against a comment reading "when this is
wired to `fs::rename`". **The hard half was done and the easy half was
missing** -- which is the stranded-serialiser shape, one level up: not a
serialiser with no door, but an entire correct algorithm with no filesystem
under it.

**What changed:**

* `Ctrl+O` opens a folder through `guitk::dialog::FileDialog::select_folder`.
  Until one is chosen the list is empty and the status line says so. The six
  invented files are now a `#[cfg(test)]` fixture, which is what they always
  were in substance.
* `perform` walks the plan calling `std::fs::rename`, updating each entry only
  when its own rename succeeded, so the list keeps describing the directory.
  A failure does not stop the batch: the plan's *order* is what makes it safe,
  so abandoning it midway is what creates the collision it was built to avoid.
* `describe` reports both halves -- "Renamed 3 file(s); 2 failed. a -> b: ..."
  -- rather than a count that hides the failures or an error that hides the
  successes.
* Undo and redo go through the same `perform`, and push onto the opposite
  stack **only if something actually moved**.
* `FileEntry` is keyed by `raw_name: OsString`, the name as the filesystem gave
  it, with the text form used for the rules and the display. A name that is not
  valid UTF-8 is listed, marked unrenameable and left unticked, because every
  rename rule reads text and writes text: renaming from a lossy form would
  write a name **nobody asked for**, since `to_string_lossy` substitutes U+FFFD
  and that is a different name.
* `original_path: String` and its `replace_file_name` helper are gone. The path
  is `folder.join(raw_name)`, derived, so the class of bug that helper existed
  to fix -- `path.replace(old, new)` rewriting a *directory* whose name
  contains the file's name -- cannot occur at all now.

**The tests changed more than the code did, and that is the finding.**
`app_with(&["a.txt", "b.txt"])` used to build a `Vec` of names and no files.
Every test that called `execute_rename` was therefore checking a memory shuffle
-- against a program whose status line said it had renamed things. They now
build a real scratch directory, and `on_disk()` reads the folder back. Two
consequences surfaced immediately:

1. `test_app_execute_rename` went red, because with no folder open nothing is
   renamed and nothing is pushed to the undo stack. **That is the defect, found
   by its own test suite the moment the suite was made to touch a disk.**
2. `a_case_only_rename_is_not_a_conflict` could not be given a real fixture at
   all. It writes `photo.JPG` and `PHOTO.jpg`, and **the host these tests run
   on is Windows, whose filesystem is case-insensitive**, so the two collapse
   into one file. The rule under test is a property of the *target*
   filesystem, which `design.txt` specifies as case-sensitive, and the host
   cannot hold the fixture that would demonstrate it. It keeps a name list,
   with the reason written at the test, because conflict detection is a
   function of the names alone.

The fixture guard -- `assert_eq!(app.files.len(), names.len(), "the fixture did
not load, or this test asserts nothing")` -- is what turned (2) from a silently
weaker test into a failure with an explanation. That is the same guard lane B
found five copies of in `userspace/`, and it earned its place again here.

**Verified by sabotage**, eight claims, each broken with an edit that still
compiles: the rename silenced, a failure counted as a success, a non-text name
treated as text, the picker undrawn, an undo queued for a rename that did not
happen, and folders listed as files. All eight went red.
