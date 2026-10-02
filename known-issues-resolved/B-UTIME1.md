### B-UTIME1. Linux-ABI `utimensat`/`utimes`/`utime` returned EROFS unconditionally (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could update a file's access/modification
timestamps. Every `utimensat(2)`/`utimes(2)`/`utime(2)` returned `-EROFS`
after the (Linux-faithful) input-shape gate ladder, even though the VFS is
writable and `Vfs::set_times` is implemented by memfs, ext4 and fat. `touch`,
`make` (which stamps targets), `tar -x` (restores mtimes) and configure
scripts all depend on this.

**Root cause:** The three handlers were placeholder stubs that validated their
arguments (and faithfully reproduced Linux's `EINVAL`/`ENOENT`/(OMIT,OMIT)→0
input-shape diagnostics — batch 489) and then hard-coded
`linux_err(errno::EROFS)`. The EROFS terminal is also asserted by the
batch-489 fidelity self-tests, which call the handlers in kernel context, so
the fix had to keep that terminal for kernel callers.

**Fix (proper):** For ring-3 callers (`caller_pid().is_some()`) each handler
now resolves the target path (`resolve_at_path` against the caller's cwd /
dirfd; `utimensat` with a NULL pathname resolves the open file behind `dirfd`
via `handle_path`), translates the parsed `timespec`/`timeval`/`utimbuf` into
ns-since-epoch (`UTIME_NOW`→`clock_realtime`, `UTIME_OMIT`/NULL-field→leave
unchanged, otherwise `sec*1e9 [+ sub-second]`), requires a File-WRITE
capability, and calls `Vfs::set_times`. Kernel context preserves the EROFS
terminal. Regression test: Path Z Part 29 (`self_test_linux_utimensat`) — a
hand-built raw-syscall ELF (`build_linux_utimensat_test_elf`) calls
`utimensat(AT_FDCWD, "/utimensat-test", {atime=1.6e9 s, mtime=1.5e9 s}, 0)`
from ring 3 (self-diagnosing exit sentinel `0xD1`); the harness stages the file
on the memfs root (memfs implements `set_times`) and, after exit 0,
independently asserts the kernel-side `Vfs::metadata` reports
`accessed_ns == 1.6e18` and `modified_ns == 1.5e18` exactly.

**Fidelity gaps (minor, documented in the linux.rs module comment):**
1. ~~`Vfs::set_times` always follows symlinks, so `utimensat`'s
   `AT_SYMLINK_NOFOLLOW` is a no-op.~~ **RESOLVED 2026-07-22** alongside
   B-CHOWN1 gap #1: added `Vfs::set_times_no_follow` (+ `FileSystem`
   `set_times_no_follow` with memfs/ext4 overrides); `utimensat`/`lutimes`
   now stamp the link inode itself when `AT_SYMLINK_NOFOLLOW` is set. See the
   B-CHOWN1 gap #1 resolution note for the full plumbing.
2. The `Timestamp = u64` VFS API overloads `0` ("ns since epoch") as the
   "leave this field unchanged" sentinel, so a request to set a field to
   exactly the Unix epoch (or any pre-epoch / negative instant) is silently
   treated as "leave unchanged". Proper fix needs an `Option<u64>` (or
   explicit "omit" flag) plumbed through `Filesystem::set_times` for every FS.
