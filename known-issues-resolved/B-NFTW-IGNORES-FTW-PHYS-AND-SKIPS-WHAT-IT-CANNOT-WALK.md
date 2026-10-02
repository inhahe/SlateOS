### B-NFTW-IGNORES-FTW-PHYS-AND-SKIPS-WHAT-IT-CANNOT-WALK. Four defects in one walker — 2026-09-04 — FIXED 2026-09-04

**Where:** `posix/src/ftw.rs`. Found by auditing `ftw`/`nftw` for the same
defect family as `B-FTS-SWALLOWS-EVERY-DESCENT-IT-CANNOT-MAKE` immediately
after fixing it. The family was there, and worse.

| Defect | Old behaviour | Now |
|---|---|---|
| `FTW_PHYS` never even read | `nftw` destructured `flags` for `FTW_DEPTH` only. `lstat` appears nowhere in the old file, so `nftw(p, cb, n, FTW_PHYS)` called `stat` — it followed every symlink it was told not to follow, descended into symlinked directories, and handed a recursive-delete caller `FTW_D` for a symlink. `FTW_SL` and `FTW_SLN` were likewise defined and never produced | `lstat` when physical; a link reports `FTW_SL` and is not descended into; a dangling link without `FTW_PHYS` reports `FTW_SLN` |
| `FTW_DNR` never produced | `opendir` failure was `return 0` — an `EACCES` directory vanished from the walk, and a `du` built on this under-reported in silence | The directory is opened *before* it is announced; a refusal reports `FTW_DNR` in place of `FTW_D` |
| Depth limit silent | Past `nopenfd`/`MAX_DEPTH`, `return 0` — and with `FTW_DEPTH` the `FTW_DP` still fired, so a recursive delete removed a directory it had never emptied | `FTW_DNR` with `errno` = `ENOMEM`, and no `FTW_DP` |
| Over-long child path skipped | `continue` — a truncated traversal reported as a complete one | The walk ends with -1 and `ENAMETOOLONG`; POSIX has no per-entry flag for this |

Plus two structural problems that are why the above happened:

- **`ftw` and `nftw` were two near-identical recursions** (`ftw_recurse` /
  `walk_directory` / `nftw_recurse` / `nftw_walk_directory`). Four copies of
  one loop is how three of them ended up handling a case the fourth did not,
  and how a flag only `nftw` accepts ended up implemented in neither. They are
  now one `Walker<F>` differing only in how an entry is delivered, so a walk
  option cannot be honoured in one entry point and dropped in the other.
- **The module header described behaviour the module did not have** —
  "symbolic link detection depends on `lstat` (uses `stat` as fallback)",
  written about a file containing no call to `lstat`. A doc comment is not
  evidence; this one is the reason the gap survived, since a reader checking
  whether symlinks were handled found an answer without reading the code.
- **The path was a `[u8; PATH_MAX]` local in the recursive function** — 4 KiB
  of stack per level, 128 KiB at the depth cap, while the module header cited
  stack overflow as the *reason* for the cap. One buffer now lives in the
  `Walker`, mutated in place as the walk descends and ascends.

`FTW_MOUNT` and `FTW_CHDIR` were also accepted and ignored; they are now
refused with `EINVAL` (`UNSUPPORTED_NFTW_FLAGS`). See design-decisions.md
§761 for why refusing beats approximating, and for the one behaviour change a
caller could notice: a walk that used to return 0 having omitted a subtree may
now return -1.

**Reproduce (before the fix):** no in-tree caller exists — `git grep` finds
`ftw`/`nftw` only in this module and its tests; both APIs are for ported C
code, which is why four defects sat in one file undisturbed. The `FTW_DNR`
case is provable from the source alone: the constant was defined, its numeric
value asserted in `test_ftw_type_flags`, and never assigned anywhere.

**Tests:** `test_ftw_null_path_efault`, `test_nftw_null_path_efault`,
`test_ftw_zero_nopenfd_einval`, `test_nftw_zero_nopenfd_einval`,
`test_nftw_rejects_ftw_mount`, `test_nftw_rejects_ftw_chdir`,
`test_nftw_accepts_the_flags_it_implements`,
`test_run_rejects_an_over_long_root`, and the five
`test_append_component_*` cases including
`test_append_component_overwrites_the_previous_sibling` (the in-place buffer's
one new hazard: a shorter sibling name must not leave the tail of a longer one
behind it).

**Not covered by tests, and why:** the walk itself. Every raw syscall returns
`HOST_ENOSYS` on the host (`posix/src/syscall.rs`), so `stat` and `opendir`
both fail and the root is always reported `FTW_NS`. The reporting paths that
matter — `FTW_DNR`, `FTW_SL`, `FTW_DP` ordering — need a filesystem, and are
therefore reachable only from a booted target. Worth stating plainly, because
the test count says otherwise: the tests above prove the *argument handling*
and the path arithmetic, and nothing at all about the traversal. All four
defects here were found by reading, not by a failing test, and a fifth of the
same kind would have to be found the same way.
