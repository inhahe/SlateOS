### [D] B-D-FTW-STOPS-AT-NOPENFD-DEEP — 2026-09-26 — FIXED 2026-09-26

**Fix.** `ftw` and `nftw` are glibc 2.39's io/ftw.c now (design-decisions
§1109). Each level reads its directory's names into memory and closes the
stream before descending — glibc's spill, done eagerly, which costs nothing
extra because our `DIR` is already a snapshot — so the walk holds one
descriptor at a time at any depth, and `nopenfd < 1` is 1. Without `FTW_PHYS`
every directory entered is recorded by `(st_dev, st_ino)` and one entered
before is skipped, as glibc's `find_object` does; that is what stops a
symlink cycle now that `MAX_DEPTH` is gone. On the way, the rest of
`ftw_startup`/`process_entry`/`ftw_dir`: an empty root is `ENOENT`, a root
that cannot be `stat`ed gets no callback, trailing slashes come off the root,
a `stat` failure other than `EACCES`/`ENOENT` and an `opendir` failure other
than `EACCES` end the walk, an unknown `nftw` flag is `EINVAL`, and
`FTW_ACTIONRETVAL` works. The walker reaches the filesystem through a small
trait, so the host tests walk trees of their own — the walk itself had no
tests at all, because on the host every path is missing.

**Where:** `posix/src/ftw.rs` — `ftw`, `nftw` (the `Walker`).

**In short:** `ftw(root, fn, n)` and `nftw` visit only the top `n` levels of a
tree, and never more than 32: a directory deeper than that is reported as
unreadable (`FTW_DNR`, `errno` `ENOMEM`) and nothing inside it is visited.
glibc walks the whole tree whatever `n` is; `n` only limits how many
directories it holds open at once. A program passing a small `n` — `ftw(dir,
fn, 1)` is common, 20 traditional — silently misses files, and a traversal
that omits files is worse than one that fails.

**Repro:** a tree `a/b/c/f`; `ftw("a", cb, 1)` reports `a` as `FTW_D` and
`a/b` as `FTW_DNR`; `c` and `f` are never seen.

**Why.** The walker holds one directory stream open per level, so the
descriptor budget is also a depth budget (`depth_limit`, capped by
`MAX_DEPTH`). Found by the NULL-pointer audit's eleventh pass.

**Proper fix.** glibc's (io/ftw.c, `open_dir_stream`): when a new level needs a
stream and `n` are open, read the rest of the oldest open stream's entries into
memory, close it, and serve that level from memory when the walk returns to it.
Then no depth cap is needed but `PATH_MAX`'s, and `nopenfd < 1` becomes 1, as
glibc's `ftw_startup` makes it, instead of `EINVAL`. Symlink cycles without
`FTW_PHYS` then need glibc's other half, the set of directories already walked
(`find_object`), since `MAX_DEPTH` is what stops them today.
