### B-FTS-DROPS-THE-65TH-CHILD-OF-ANY-DIRECTORY. `find`, `rm -rf`, `du` and `chmod -R` silently skipped files — 2026-09-04 — FIXED 2026-09-04

**Where:** `posix/src/fts.rs`, the (now removed) `snapshot_dir` /
`CachedChild` / `DirFrame::{children, n_children}` machinery, and
`FTS_NAME_MAX`.

**Two separate silent losses, in the same function:**

| # | Input that triggered it | What happened | What the caller saw |
|---|---|---|---|
| 1 | A directory with more than 64 entries | Entries past the 64th were never copied into the frame | Traversal completed successfully, missing files |
| 2 | A component name longer than 63 bytes | The entry was skipped in the copy loop | Same — success, missing file |

Neither produced an `FTS_ERR` entry, an `errno`, or a diagnostic. The module
docs claimed loss (2) was "skipped with an `FTS_ERR` entry"; it was not, so
the documentation actively concealed the defect. `fts` is what `find(1)`,
`rm -rf`, `du` and `chmod -R` are built on, so "silently" meant a recursive
delete that reported success while leaving files behind, and a `du` that
under-reported.

**Why the eager copy existed, and why the reason had already expired:** the
frame buffered a directory's whole listing so the underlying `Dir` handle
could be released before descending, back when `dirent`'s pool held 8 streams
and each slot carried a 68 KiB inline buffer. `dirent.rs`'s own docs
(lines 1011-1013) say so outright: the small pool "is what forced
`crate::fts` to buffer children eagerly." That pool is now 64 slots, which
covers `MAX_FTS_INSTANCES * MAX_FTS_DEPTH` = 16 with room to spare.

`fts.rs` had also acquired a *second*, incorrect justification for the copy —
that it "makes a frame independent of a stream that could be invalidated
underneath it." That is false: `Dir` slurps the complete listing at `opendir`
time via `SYS_FS_GETDENTS_PINNED` and never re-reads, so holding a stream open
is exactly as independent of later mutation as copying its entries was. The
copy bought nothing and cost every 65th file. This is the failure mode where a
constraint is removed but the workaround it forced stays, acquires a
plausible-sounding rationale, and outlives the fact that justified it.

**Fix:** each `DirFrame` now holds its own `crate::dirent::Dir` open for the
frame's whole life and pulls the next entry on demand (`open_frame`,
`next_child`, `close_frame`, `close_open_frames`), so there is no per-directory
cap at all. `FTS_NAME_MAX` went 64 -> 256 to match the new
`dirent::DIRENT_NAME_MAX`, which is the capacity of the `d_name` field
`readdir` fills — so a name `readdir` can produce now always fits a frame, and
a `const _: () = assert!(...)` fails the build if either relationship is ever
broken again. Stream ownership is released on the pop path
(`pop_and_yield_dp`) and by `close_open_frames`, which `release_instance`
calls *before* it resets the slot; `DirFrame` is `Copy` and lives in a
`static` pool, so it has no drop and those two call sites are the whole of the
ownership discipline.

**Tests:** seven added to `fts.rs`'s pure-logic `mod tests` -
`a_traversal_at_full_depth_cannot_exhaust_the_dir_pool`,
`every_name_readdir_can_produce_fits_a_frame`, `a_fresh_frame_owns_no_stream`,
`closing_a_frame_twice_does_not_close_twice`,
`a_frame_with_no_stream_yields_no_children`,
`closing_the_open_frames_empties_the_stack`, and
`a_frame_no_longer_carries_a_listing`, which asserts
`size_of::<DirFrame>() < 1024` on the grounds that a frame that large is a
listing buffer wearing a frame's name.

See design-decisions.md §759.
