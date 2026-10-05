## 1103. `fts` has glibc's ABI and BSD's algorithm, and never changes directory

**Date:** 2026-09-25
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** `fts` is the C library's "walk a directory tree" interface, the one
BSD and GNU `find`, `rm -r` and `du` are written against. Ours could go only
eight folders deep, walked only the first folder it was given, ignored the
requested sort order, and — worse — laid out its data and numbered its
commands differently from Linux's, so a Linux-built program using it would have
misread every result and had its "skip this folder" requests silently ignored.
It is now a faithful re-implementation of the BSD design Linux's own uses, with
Linux's exact data layout. Nothing in the tree calls it today; this is so that
the first ported program that does gets what it expects.

**What was wrong.**

| | before | now |
|---|---|---|
| Depth | 8 levels (one open stream per level, from a pool of 64) | unlimited: a directory is read in one pass and released, as in BSD |
| Streams | 2 at once | any number (heap objects) |
| Roots | only the first of `argv` | all, in `argv` order or sorted by `compar` |
| `compar` | ignored | sorts roots and every directory |
| `fts_children` | `ENOSYS` | real, including `FTS_NAMEONLY` |
| `FTS_SEEDOT`, `FTS_XDEV` | ignored | honoured |
| Cycles under `FTS_LOGICAL` | walked until the depth limit | `FTS_DC`, with `fts_cycle` set |
| `FTSENT`/`FTS` layout | this crate's own | glibc's, pinned by offset in tests |
| `FTS_AGAIN`/`FOLLOW`/`NOINSTR`/`SKIP` | 2/1/4/3 | glibc's 1/2/3/4 |
| Tests of the traversal itself | none — the host has no filesystem | a walk over an in-memory tree, through an `FsOps` seam |

**The choices, and what they cost.**

- **Never `chdir`.** glibc changes directory as it descends, so that each access
  path is short. That makes `fts` unsafe in a threaded program, and this libc's
  working directory is process state other threads read. Here every walk is
  `FTS_NOCHDIR` (reported in `fts_options`) and `fts_accpath == fts_path`.
  Cost: paths are full paths, so a tree deeper than `PATH_MAX` gives `FTS_NS`
  entries past that depth where glibc would carry on — the same limit `ls -R`
  and every path-based tool here already has.
- **`fts_open` requires exactly one of `FTS_LOGICAL`/`FTS_PHYSICAL`**, as the
  manual page says, rather than glibc's silent default.
- **`fts_set` returns 1 on error**, glibc's actual value, not the manual page's
  -1; callers test for non-zero.
- **An empty root list is a walk that ends at once**, as in glibc; the old code
  refused it with `EINVAL`.

**Revisit when** a ported program needs `chdir`-speed on very deep trees (it
would need a thread-safe design, e.g. `openat`-relative walking with per-level
descriptors), or `FTS_WHITEOUT` gains meaning.
