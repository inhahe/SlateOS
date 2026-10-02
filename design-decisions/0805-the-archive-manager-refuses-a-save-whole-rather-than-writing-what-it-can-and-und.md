## 805. The archive manager refuses a save whole rather than writing what it can, and undoes a refused edit from memory rather than from the file

**Date:** 2026-09-02
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the Archive Manager can now change a ZIP file — Add puts a file in,
Delete takes members out. Both work by writing a *new* archive containing
everything that should still be there, and putting it in the old one's place.
That raises a question the reading half never had to answer: what should happen
when one of the members that should still be there cannot be reproduced? The
decision is that the whole operation is refused and the file is left exactly as
it was, even though this means one unreadable member makes the archive
un-editable. And when a refusal has to put back rows the user already saw
disappear, the rows come from a copy kept in memory, not from re-reading the
file.

### The first decision: refuse whole, don't write what you can

There is no in-place edit of a ZIP. Removing a member means writing a new
archive without it, so every member that stays has to be decompressed out of the
old file and compressed back into the new one. A member that will not come back
out — it is encrypted and this build has no decryption, or it fails its own CRC
check — cannot be put into the new archive.

| | Write what can be written | Refuse the whole save |
|---|---|---|
| *What changes:* | Delete succeeds; the encrypted member is silently gone from the file afterwards | Delete reports "cannot be rewritten — it is encrypted…; nothing was changed", and the file is untouched |
| Cost when it fires | the user loses a file they never asked to delete, inside an archive they still believe is intact | the user cannot edit this archive at all until they extract it and rebuild it elsewhere |
| When they notice | possibly never | immediately |

The second column's cost is real and it is not small: an archive with one
encrypted member becomes read-only in this program. It was chosen anyway,
because the first column's cost is *unbounded and silent*. A partial success in
a rewrite is not a partial success; it is data loss reported as completion. The
one thing a user cannot recover from is not knowing it happened.

A "skip it and warn" middle option was rejected for the same reason. A warning
on the status line is a line of text that scrolls away, attached to an operation
that already succeeded — and the file is already rewritten by the time it is
read.

### The second decision: undo from memory, not from the file

Delete removes the rows from the list first and then saves. When the save is
refused, the rows have to come back, or the window describes an archive that
still contains them and the user closes the program believing a deletion
happened.

The obvious way to put them back is to re-read the archive from disk — it is
guaranteed unchanged, and re-reading is what the *success* path does anyway.
That was the first implementation and it is wrong, for a reason that only shows
up in exactly the case it is needed: **the reasons a write fails are largely the
reasons a read fails.** A removed drive, a file deleted underneath the program, a
permissions change — each fails the rewrite and then fails the recovery read too,
and `open_path` keeps the previous archive on a failed read, which is the
*edited* list. The recovery would silently do nothing, in the one situation it
exists for.

So `AppState::save` takes the pre-edit entry list as an argument and restores it
on refusal. A restore from memory has no failure mode. On success the file *is*
re-read, because there the re-read is not recovery — it is the only way to learn
the new sizes, ratios and member ids, all of which the rewrite changed.

The same reasoning gives the success path its own honest failure message: if the
write succeeded and the re-read then failed, the status line says both ("Saved …
— but it could not be read back, so this list is out of date. Re-open it"),
because "Saved" alone describes the file correctly and the window wrongly.

### What was not decided here

**Immediate save vs. a Save button.** Add and Delete write the file at once,
matching 7-Zip and WinZip. This is not really a tradeoff — a deferred model
would need a dirty-state indicator, a close-confirmation prompt and a way to
discard, none of which exist — but it is recorded because it is the thing a
reader would otherwise assume was overlooked rather than chosen.

**Where it is:** `apps/archivemanager/src/backend.rs` — `save`, `SaveError`
(every variant of which leaves the file untouched), `replace_file` (write beside,
rename over); `apps/archivemanager/src/main.rs` — `AppState::save`'s `undo`
parameter, `ArchiveModel::set_entries`, `open_path`'s new `bool` return, and the
`can_write` arm of `toolbar_enabled` that disables both write buttons when the
open model has no bytes behind it.
