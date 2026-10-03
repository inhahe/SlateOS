## The file explorer's paste and delete were a weaker duplicate of its own engine (lane C)

**Status: FIXED 2026-08-15** (lane C, commit `bcd1e2d5d`). Found while auditing
`to_string_lossy` uses in `apps/explorer` — those turned out to be fine (the
real `PathBuf` is always kept as the truth and the lossy `String` is only ever
displayed), but the surrounding code was not.

`apps/explorer/src/fileops.rs` is a complete file-operation engine: plans with a
conflict policy, a crash-recovery journal, per-file error policy, progress, an
undo stack, and a `RecycleBin` that stores each item with its original path so
it can be listed and restored. `apps/explorer/src/main.rs` used **none of it.**
`paste()` and `delete_selected()` called `fs::copy` / `fs::rename` /
`fs::remove_file` in a loop, discarding every `Result` with `let _ =`. Three
distinct silent failures followed:

- **Paste destroyed an existing file of the same name.** `fs::copy` overwrites
  its destination, so pasting `notes.txt` into a folder that already had one
  replaced it with no prompt, no rename, no undo.
- **"Move to recycle bin" produced files that could not be restored — and
  destroyed each other.** It renamed into a flat `/var/recycle` with no
  metadata, so `RecycleBin::list` never saw the item and `restore` had no
  original path to restore *to*. And because `fs::rename` overwrites, deleting a
  second `notes.txt` from a different directory silently destroyed the first
  one already sitting in the bin. The recycle bin was, in effect, a shredder
  with an unreliable name-collision hazard.
- **The status line reported unconditional success.** "Paste complete" whether
  or not any file copied; "N item(s) deleted" where N was the number
  *selected*, not the number that worked.

The fix routes both through the existing engine rather than patching the
duplicate — `copy_dir_recursive` is deleted, not repaired. Two implementations
of one operation is precisely how the weaker one ends up on the user-facing
path; `CLAUDE.md`'s "watch for band-aid accumulation" rule names this shape.

**A fourth defect surfaced only when the tests were written.** Every operation
ends by calling `load_directory()`, which calls `update_status()`, which
overwrote `status_message` with the folder/file summary. So no operation result
was *ever* visible to the user — not a paste, not a delete, not a rename, and
not the `Error: {e}` set when `read_dir` fails, which was assigned and then
discarded two lines later (an unreadable directory rendered as "0 folder(s), 0
file(s) — 0 B"). The transient result and the derived summary are now separate
fields, with `status_bar_text()` preferring the result and navigation clearing
it.

**The root cause behind all four: `apps/explorer/src/main.rs` had no
`#[cfg(test)]` module at all.** `columns.rs`, `dropzone.rs`, `fileops.rs` and
`thumbs.rs` are all well tested; the file holding `ExplorerState` — navigation,
selection, clipboard, paste, delete, rename — had zero tests. It now has 11,
with each of the three behavioural fixes verified non-vacuous by a separate
break. Worth generalising: **a well-tested support module is not evidence that
the code calling it is tested**, and in this crate the untested file was the one
users actually touch.

Two smaller defects in `fileops.rs` noticed during the same read — **also FIXED
2026-08-15**, commit `35f17dfd7`:

- `RecycleBin::recycle` moved data with a bare `fs::rename`, which fails with
  `EXDEV` across a mount point — so deleting anything from a separate data
  partition simply errored, and `restore` had the same problem in reverse. Now
  routed through a `move_path` helper that tries the rename and falls back to
  copy-then-remove. Note that `fileops::same_device` exists for exactly this
  check and was referenced only by its own test (`dropzone.rs` carries a second
  copy of the same function); it was not used here, because attempting the
  rename and reacting to its failure is both cheaper in the common case and
  correct where a first-component heuristic guesses wrong.
- `RecycleBin::recycle` wrote the original path to `meta.txt` with
  `path.display()`, which is lossy, and `read_entry` parsed it back as UTF-8 —
  so a non-UTF-8 path was restored under a *different name*, silently renaming
  the user's data during an operation advertised as reversible. Same class as
  the `u8 as char` section above, reached through `Display` instead. The path is
  now percent-encoded from `OsStr::as_encoded_bytes`, with a version marker on
  line 1 so an already-populated bin is still readable.

A third, unlogged defect fell out of writing those tests: metadata was written
before the data was moved but not removed if the move failed, so a failed
recycle left the bin listing an entry whose `data/` was not there. Ordering
kept (metadata first is the safe order — orphaned metadata is harmless, moved
data with no metadata is unrestorable), with cleanup on the failure path.
