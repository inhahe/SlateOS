## A-VFS-EIGHT-MORE-AT-CALLS-STILL-RESOLVE-BY-PATH (lane A)

**Status:** OPEN 2026-08-30

Three of the `*at` family — unlink, stat, list — now have kernel primitives
that resolve the *handle* rather than its name: `SYS_FS_UNLINKAT_PINNED` (662),
`SYS_FS_FSTATAT_PINNED` (663), `SYS_FS_GETDENTS_PINNED` (664), backed by
`Vfs::unlink_at_pinned` / `metadata_at_pinned` / `readdir_pinned` in
`kernel/src/fs/vfs.rs`. See design-decisions.md §647.

The other eight do not: `renameat`, `linkat`, `symlinkat`, `mkdirat`,
`fchmodat`, `faccessat`, `readlinkat`, `utimensat`. They still reach the kernel
as an already-concatenated path from `posix/src/file.rs::resolve_dirfd_path`,
carrying the full defect this whole thread is about.

**Why three and not eleven.** Lane B named exactly these three as what a
recursive tool actually touches, and building the other eight speculatively
would be eight ABI surfaces designed against no caller. The machinery
generalises — `PinnedDir`, `Vfs::pin_dir`, `verify_pinned` and `check_at_name`
are the whole of it, and each additional call is a short wrapper following
`unlink_at_pinned`'s two-pass shape.

**Proper fix.** Add them as lane B names them. The pattern to copy is
`Vfs::unlink_at_pinned`: verify once before any side-effecting policy step,
then verify *again* inside the same filesystem-lock hold as the operation. A
version that verifies only once, outside the acting guard, narrows the race
instead of closing it and will read as fixed — that is the trap.

**Trigger:** a request from lane B naming which ones, or a ported tool that
needs one.
