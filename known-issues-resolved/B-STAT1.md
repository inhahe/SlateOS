### B-STAT1. Path-based `stat`/`lstat`/`newfstatat`/`statx` always returned ENOENT (no-file skeleton-FS stub) — FIXED 2026-06-16

**Symptom:** Every path-based stat syscall returned `-ENOENT` unconditionally,
even for files that exist in the VFS. Any program that stats a path before
opening it saw the file as missing: dash's `[ -f FILE ]` / `[ -e FILE ]` /
`[ -d DIR ]` test predicates were always false, `ls FILE`, `stat FILE`, and
`configure`-style existence probes all failed. `fstat` (fd-based) worked, so
this only bit the path-based variants. (Distinct from B-CWD1, which was about
*relative* path resolution — B-STAT1 returned ENOENT even for valid
*absolute* paths.)

**Root cause:** The handlers carried a stale "no files exist on our skeleton
FS" assumption from before the VFS held real files. `stat_path_impl`
(shared by `stat`/`lstat`) and the non-empty-path branches of
`sys_newfstatat` / `sys_statx` validated the path pointer and then hard-coded
`linux_err(errno::ENOENT)` with comments explaining that `filename_lookup`
"always fails on our no-file FS". That was true when written but became a
silent lie once the VFS gained a backing store.

**Fix (proper):** Do a real VFS lookup for ring-3 callers.
`stat_path_impl` was rewritten into `stat_path_common(path_ptr, statbuf_ptr,
follow)` which canonicalises the path against the caller's cwd
(`canonicalize_path` + `pcb::get_cwd`) and resolves it via new helpers
`stat_meta_for_path` (calls `Vfs::metadata` when `follow`, else
`Vfs::lmetadata`; maps `NotFound`→ENOENT) + `fill_stat_from_meta` /
`fill_statx_from_meta` (map `EntryType`→`S_IF*` bits, synthesise default
perms `0o755`/`0o777`/`0o644` when the FS reports `permissions == 0`, and
backfill 0 timestamps with `clock_realtime()`). `sys_newfstatat` / `sys_statx`
non-empty branches resolve via `resolve_at_path(dirfd, path)` (dirfd + cwd
rules) then real-stat-and-fill, with `follow = (flags & AT_SYMLINK_NOFOLLOW)
== 0`. Statbuf pointer gates are deferred until *after* a successful lookup,
matching Linux's `getname`/`filename_lookup`-before-`cp_new_stat` ordering
(so `stat("missing", NULL)` returns ENOENT, not EFAULT). Kernel context
(`caller_pid().is_none()`) still returns ENOENT, preserving the batch-488
syscall-fidelity self-tests (which pass a fake pointer in kernel context).
Regression test: Path Z Part 24 (`self_test_linux_real_glibc_shell_statpath`)
runs `[ -f /bin/dash ] && echo HASFILE > /stat-out.txt` in ring 3 and asserts
the redirect fired (8 bytes, exit 0), proving the `-f` predicate's path stat
now succeeds.
