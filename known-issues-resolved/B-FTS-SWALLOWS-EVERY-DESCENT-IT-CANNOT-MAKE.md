### B-FTS-SWALLOWS-EVERY-DESCENT-IT-CANNOT-MAKE. An unreadable or too-deep directory was skipped without a word — 2026-09-04 — FIXED 2026-09-04

**Where:** `posix/src/fts.rs`, `fts_read_inst`'s "previous entry was an
`FTS_D`" branch and `descend_into_current`'s error path.

Found while fixing `B-FTS-DROPS-THE-65TH-CHILD-OF-ANY-DIRECTORY`, in the same
function, and it is the same defect wearing different clothes: a traversal that
cannot visit something reports success anyway.

**Two paths, both silent:**

1. **`opendir` failed** — typically `EACCES`. `descend_into_current` set
   `inst.current.fts_info = FTS_DNR` and `fts_errno`, and then returned to a
   caller that fell through to `step()`. `step()` reads the parent's next child
   and writes it into `inst.current` — the single entry record the whole module
   shares. So the `FTS_DNR` mark was overwritten before any caller could read
   it. The code that set it looks correct in isolation, which is why it
   survived; it was wrong only in what happened next.
2. **The traversal stack was full** (`inst.depth == MAX_FTS_DEPTH`, 8). The
   descent was simply not attempted, with a comment claiming this "matches
   BSD's deeply-nested behavior of skipping deeper entries silently." BSD has
   no such behaviour — glibc's `fts` has no depth limit at all. Everything
   below level 8 vanished *and* the directory's own post-order `FTS_DP` never
   arrived, so a `rm -r` built on this deleted the contents it could see,
   never got the callback that removes the directory itself, and exited 0.

**Fix:** both failures now re-yield *the same entry*, re-typed `FTS_DNR` with
an `errno` — which is what glibc's `fts_read` does when `fts_build` returns
NULL. That shape is forced by the architecture rather than chosen: `current`
is the one entry record, so a failure can only be reported by returning it,
never by marking it and stepping on. `descend_into_current` now returns
`Result<(), i32>` and does not touch `current` at all — only the caller is in
a position to return, so only the caller decides what the user sees. The
depth refusal moved into `descent_refusal(depth) -> Option<i32>`, which yields
`ENOMEM`: running out of traversal stack says nothing about the directory, and
`EACCES` would send someone to check permissions that are fine.

**Tests:** `a_failed_descent_pushes_no_frame` (the Err path leaves depth and
the frame stack exactly as they were — which is what makes the caller's early
`return` safe), `a_descent_within_the_depth_limit_is_not_refused`, and
`the_depth_limit_is_a_resource_error_not_an_access_error`.

**Still true, and still a limit:** the depth cap is 8, and a tree deeper than
that cannot be walked. It is now loud rather than silent, which is the part
that mattered; raising it is bounded by `dirent`'s 64-slot `Dir` pool
(`MAX_FTS_INSTANCES * MAX_FTS_DEPTH` streams are held at full depth) and is
tracked as `TD-B-FTS-DEPTH-IS-CAPPED-AT-EIGHT` below.
