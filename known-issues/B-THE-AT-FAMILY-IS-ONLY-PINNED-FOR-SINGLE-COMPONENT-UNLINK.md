## B-THE-AT-FAMILY-IS-ONLY-PINNED-FOR-SINGLE-COMPONENT-UNLINK (lane B, 2026-08-30) — PARTIALLY FIXED

`posix::unlinkat` now takes `SYS_FS_UNLINKAT_PINNED` (662), which has the
kernel resolve the directory *handle* rather than the path the handle had when
it was opened. Three gaps remain, all of them deliberate, all of them still
carrying the original TOCTOU. Recorded together because they will be closed by
the same kind of work and it should be obvious how much of the problem is left.

**1. `unlinkat(dirfd, "sub/x")` is still resolved by path.** 662 accepts a
single component only — by design; a handle-identity check proves nothing if
the name may then climb out of the directory. So a multi-component name still
goes through `resolve_dirfd_path` and is exactly as racy as it was. The proper
fix is for `unlinkat` to walk it itself: `openat2` each intermediate directory
with `RESOLVE_BENEATH`, pin the last, unlink there — which needs a descriptor
per component and an unwind that closes them all on every exit path, including
the error ones. Not a large change; it was left out so that wiring 662 was one
reviewable thing. Where: `posix/src/file.rs`, `unlinkat` / `try_pinned_unlinkat`.

**2. `fstatat` is not pinned at all, and cannot be yet.** `SYS_FS_FSTATAT_PINNED`
(663) writes the 64-byte `FS_META_SIZE` record, which has no inode number, no
hard link count and no block count. `struct stat` needs all three. Wiring it
would make `st_ino` zero for every file, which is not an error anyone checks —
it silently breaks `cp`'s same-file refusal, `ls -i`, hardlink coalescing in
`du`/`tar`, and `find -samefile`. This matters more than it looks: `rm -r`
`lstat`s every entry to decide whether to recurse, so the *stat* is the racy
step even when the unlink is not. Blocked on lane A;
`requests/b-a-662-is-wired-in-663-cannot-be-until-its-record-carries-an-inode.md`
asks for the 80-byte `SYS_FS_STAT` record instead. Where: `posix/src/file.rs`,
`fstatat`; the record layouts are in `posix/src/stat.rs` (`KERNEL_STAT_LEN`)
and `kernel/src/syscall/number.rs` (`FS_META_SIZE`).

**3. The other eight `*at` calls are untouched.** `renameat`, `linkat`,
`symlinkat`, `mkdirat`, `fchmodat`, `faccessat`, `readlinkat`, `utimensat` all
still call `resolve_dirfd_path`. Lane A has the machinery and asked us to name
which we want rather than guess; the priority order and the reasoning are in
§5 of the request above. `fchmodat` is the one that is a privilege boundary
rather than a correctness detail — `chmod -R` down a tree it does not control
is the classic symlink-swap escalation.

**Not a gap, but worth writing down:** the fallback is fail-*closed*. Only "this
kernel has no such syscall" falls back to the path route; `ESTALE`, `EACCES`
and every other verdict from 662 is final. A kernel bug that made 662 refuse
everything would break `rm` loudly rather than silently downgrade every
deletion to the unsafe route. And because `NotSupported` (-2) is ambiguous
between "unregistered slot" and "this filesystem can't", the fallback latches
off after the first call that answers with anything else — see
`design-decisions.md` §730, decision 3.
