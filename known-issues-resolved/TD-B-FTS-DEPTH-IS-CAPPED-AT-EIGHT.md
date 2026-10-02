### TD-B-FTS-DEPTH-IS-CAPPED-AT-EIGHT. `find`/`rm -r`/`du` stop at the eighth level — 2026-09-04 — FIXED 2026-09-25

**Where:** `posix/src/fts.rs`, `MAX_FTS_DEPTH`.

Eight levels is shallow for real trees: a checkout of this repository exceeds
it, and so does most of `/usr`. Since the fix above, hitting the cap produces
`FTS_DNR`/`ENOMEM` rather than silence, so a caller finds out — but it still
cannot complete the walk.

**Why it is 8:** each level holds a `Dir` stream open for the life of its
frame, and `dirent::MAX_OPEN_DIRS` is 64. A `const` assertion requires
`MAX_FTS_INSTANCES * MAX_FTS_DEPTH <= MAX_OPEN_DIRS`, so with 2 concurrent
streams the ceiling is 32 — and taking all 64 slots for traversal would starve
every other `opendir` in the process.

**What the proper fix looks like:** `dirent` already has `telldir`/`seekdir`,
and a `Dir` is an `opendir`-time snapshot, so a frame deeper than some
threshold could record its position, close its stream, and re-open + seek when
the traversal returns to it. That decouples depth from the pool entirely at
the cost of a re-`opendir` per level on the way back up, and of a directory
that may have been replaced in between — which needs the same descriptor-
identity check `rm -r`'s walk already does (design-decisions.md §752). Until
then, raising `MAX_FTS_DEPTH` to 16 (32 of 64 slots) is a cheap partial step
that halves the pool for a walk, and is not obviously the right trade.

**Update (lane D, 2026-09-25) — who is actually affected.** Nobody in the tree
today. `userspace/coreutils`'s `find`, `grep` and `mv` mention `fts_*` only in
comments that trace GNU's logic; each walks the tree itself, and no program,
port or fixture calls `fts_open` (checked with a tree-wide search). So the cap
bites only a future C program that uses `<fts.h>` — and that program would meet
more than the cap: one root only (`fts_open` ignores every path after the
first), no `compar` sorting, no `fts_children`, no `FTS_XDEV`/`FTS_SEEDOT`, and
no cycle detection under `FTS_LOGICAL`. The fix worth doing is therefore not the
`telldir`/`seekdir` juggling above but BSD's model, which the real allocator
(design-decisions §1101) now makes cheap: each frame copies its directory's
listing into a growable heap array and closes the stream at once, so depth
costs neither `Dir` slots nor descriptors; instances and the frame stack live on
the heap; and `FTS_DC` compares each new directory's device and inode with its
ancestors'. Lane D's next `fts` task, after the allocator is in.

**FIXED (lane D, 2026-09-25) — by replacing the walker, not by raising the cap.**
`posix/src/fts.rs` is now BSD's algorithm with glibc's ABI (design-decisions
§1103): a directory is read into a list of heap entries in one pass and its
stream released before `fts_read` returns, so depth costs neither `Dir` slots
nor descriptors, and streams are heap objects rather than a pool of two. A
600-level tree whose path outgrows the buffer several times is walked to the
bottom in the tests. The same change makes every root walked, honours
`compar`, `FTS_SEEDOT` and `FTS_XDEV`, implements `fts_children`, reports
symlink cycles as `FTS_DC`, and gives `FTSENT`/`FTS` and the constants glibc's
values — the instruction constants had been swapped, so a glibc-built caller's
`FTS_SKIP` was read as "no instruction".
