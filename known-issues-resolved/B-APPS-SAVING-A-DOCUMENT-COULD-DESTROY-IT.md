## B-APPS-SAVING-A-DOCUMENT-COULD-DESTROY-IT (lane C, 2026-08-16) — FIXED for the editors, tracked below for the rest

**In short:** Saving a file emptied it first and then wrote the new contents.
If anything interrupted the save in between — the disk filling up, a USB stick
pulled out, the app being killed, the power going — the file was left empty or
half-written, permanently. Saving is the operation users perform *to protect*
their work, so this turned the safety action into the thing that lost the work.
Fixed for the text editor, markdown editor and paint: they now write a
temporary file alongside and swap it into place in one step, so an interrupted
save leaves the previous version untouched.

**Where:** `std::fs::write` call sites across `apps/**`. The new shared crate
is `apps/safeio`.

**The mechanism.** `fs::write` opens with `O_TRUNC`: truncate, then write. The
old contents are gone from the moment the file is opened, and the new contents
arrive over some non-zero interval. Every byte of that interval is a window in
which a crash leaves a file that is neither the old version nor the new one.
The larger the document, the wider the window.

**The fix.** `safeio::write_atomically` writes to a temporary in the target's
own directory, `sync_all`s it, then `rename`s it over the target. `rename`
within a directory is atomic, so there is no instant at which a reader can see
a partial file, and an interruption anywhere leaves the original exactly as it
was.

**Five things that are easy to get wrong here, and what was done about each:**

| Hazard | Why it bites | Handling |
|---|---|---|
| Temporary in `/tmp` | `rename` is only atomic within one filesystem; across a mount point it silently degrades to copy-then-delete, restoring the exact bug | Temporary is created in the target's own directory |
| Symlinked target | rename-over replaces what it renames *onto*, so saving a symlinked dotfile would delete the symlink and leave a regular file | `canonicalize` first, replace the resolved file |
| Permissions | the replacement is a brand-new file created with the process's default mode, so a private file could come back world-readable | copy the original's permissions onto the temporary before renaming |
| Temporary name collisions | two concurrent saves picking one name write into each other's half-finished file | `create_new`, which fails rather than truncating, plus a per-process counter |
| Open handle on Windows | Windows refuses to rename a file that is still open, so a naive version fails *every* save on the dev host | explicit `drop(file)` before the rename |

**What it deliberately does not promise.** That the *new* contents survive a
power loss. That needs the containing directory flushed too, which Windows
cannot express (a directory cannot be opened as a file). The asymmetry is the
whole point of the trade: losing the edit you just made is recoverable by
making it again, whereas losing the file that existed *before* the save is not.

**Remaining call sites — not yet converted, deliberately.**

| Site | Assessment |
|---|---|
| `apps/installer/src/grub.rs` (4) | **Highest severity of the remainder.** A truncated `grub.cfg` is an unbootable machine, and the installer writes it exactly once with no chance to retry. Should be converted. Not done here only because the installer's write paths need reading as a whole first — they write several related files that must be consistent with each other, which is a stronger property than per-file atomicity and may want a different shape. |
| `apps/backup/src/main.rs` (6) | Manifest and metadata for a backup set. A truncated `manifest.json` makes the whole backup unrestorable, so severity is high; same caveat as the installer about multi-file consistency. |
| `apps/indexer/src/main.rs` (3) | Index + PID file. The index is a **cache**: a corrupt one costs a re-scan, not user data. Worth converting for the reduced startup pain, not urgent. |
| `apps/screenshot/src/main.rs` (1) | Writes a new file to a new name; there is no previous version to destroy. The failure mode is a truncated PNG the user can see is broken and simply retake. Low. |
| `apps/explorer/src/thumbs.rs` (1) | Thumbnail **cache**, regenerable by definition. Lowest. |

The tiering is the point: atomicity is not free (an extra file creation and an
fsync per save), and for a regenerable cache the naive write is the correct
engineering choice, not an oversight. Converting all eight uniformly would have
been the easy call and the wrong one.

**Tests:** 9 in `apps/safeio`, including `a_failed_save_leaves_the_original_untouched`
and `a_failed_save_cleans_up_after_itself` (failure injected portably by putting
a directory where the target file should be, so the final rename fails on every
platform), `concurrent_saves_do_not_share_a_temporary`, and
`repeated_saves_of_one_file_all_land`.

**Sweep note.** Sixth find, and the first that is *systemic* rather than local:
the same defect in eight places because the standard library's most obvious
function has a sharp edge that its name does not mention. Worth recording as a
lesson about where to look next — the previous five finds were all "this call
site is wrong", found by reading call sites. This one was found by asking a
different question: **which std functions are dangerous by default, and who
calls them?** That question generalises, and `fs::write` is unlikely to be the
only answer. `fs::rename` (silently overwrites) already produced two finds
today; `create_dir_all` (succeeds on an existing directory) produced one.

### UPDATE 2026-08-16 (2) — the remaining sites, and a correction

The table above tiered five remaining `fs::write` sites and deferred the
installer and backup ones on the grounds that they "write several related
files that must be consistent with each other". Having actually read them,
**that was wrong in both directions**, which is worth recording as-is rather
than quietly editing: I over-rated one and badly under-rated the other.

**Under-rated: the backup blob store.** Not in the table at all, because the
grep that produced the table found `fs::write` and this site's worse half is
`fs::copy`. `CasStore::has_blob` is `path.exists()` — existence *is* the
deduplication test, with no hash check on that path. An interrupted store left
a **truncated blob at the hash's own path**, and nothing ever rechecked it. So
every subsequent backup containing that content deduplicated against the
damaged copy and reported "already have it". A single interrupted write
silently poisons that content for every future backup, in the one application
whose entire purpose is that the data survives — and the user finds out at
restore time, which is the worst possible moment. This was the most severe
find of the whole sweep, and the tiering exercise nearly missed it because it
was framed around one function name.

**Over-rated: the backup manifest.** The claimed multi-file consistency
problem does not exist, and the existing code is already correct: `list_backups`
counts a directory as a backup only if `meta.json` is present *and* parses, and
`meta.json` is written last. So an interrupted backup leaves a directory
without a valid meta and is correctly ignored rather than offered as
restorable. That is a well-chosen completion-marker design that nothing in the
code called out. It is now documented at the call site as load-bearing, because
an invariant that only survives while nobody reorders two adjacent lines should
say so.

**Converted since:** the two GRUB script writes (`update` overwrites a *working*
boot entry; `install` leaves a partial shell script that `grub-mkconfig`
executes without complaint), the backup restore write (it writes over a file
the user still has, so an interruption destroyed the original *and* failed to
supply the replacement), the schedule list, and both CAS store paths. New
`safeio::copy_atomically` covers the copy case.

**Still deliberately not converted:** `apps/indexer` (index is a cache — a
corrupt one costs a re-scan), `apps/screenshot` (writes a new name; no previous
version to destroy), `apps/explorer/src/thumbs.rs` (thumbnail cache). The
reasoning in the original table stands for these three.

**Windows gotcha, cost about ten minutes:** `File::sync_all` on a handle opened
for *reading* fails with `ERROR_ACCESS_DENIED`. `copy_atomically` reopens the
temporary to flush it, and doing so read-only made every copy fail. Reopened
for write.

**Lesson for the rest of the sweep — the one worth keeping.** Both errors above
came from tiering by *severity of the file* rather than by *what the code does
with the file*. The manifest looked scary and was fine; a blob-store `fs::copy`
looked routine and was the worst bug found. The question that separates them is
not "how important is this file" but **"does anything downstream treat this
file's existence as a promise about its contents?"** Where the answer is yes —
a content-addressed store, a lockfile, a completion marker, a cache keyed by
hash — a partial write is not corruption of one file, it is a false promise
that propagates. Grep for `.exists()` used as a guard.
