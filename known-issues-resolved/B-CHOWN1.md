### B-CHOWN1. Linux-ABI `chmod`/`fchmod`/`fchmodat`/`fchmodat2`/`chown`/`lchown`/`fchown`/`fchownat` returned EROFS unconditionally (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could change a file's permission bits or
ownership. The whole chmod/chown family returned `-EROFS` after input
validation, even though the VFS tracks Unix mode bits and uid/gid and
implements `Vfs::set_permissions` / `Vfs::set_owner` across memfs/ext4/fat.
`install -m`, `chmod +x`, `tar -x` (restores perms/owner), and package
managers all depend on this.

**Root cause:** Placeholder stubs that validated arguments (faithfully
reproducing Linux's `ENOENT`/`EBADF`/`EINVAL` input-shape diagnostics —
batches 483/484) and then hard-coded `linux_err(errno::EROFS)`. The EROFS
terminal is asserted by those fidelity self-tests in kernel context.

**Fix (proper):** For ring-3 callers each handler resolves the target
(`resolve_at_path` for the path variants against the caller's cwd/dirfd;
`handle_path` on the open file for the `fchmod`/`fchown` fd variants and the
`fchownat(AT_EMPTY_PATH)` form), requires a File-WRITE capability, and calls
`Vfs::set_permissions` (mode masked to `0o7777`) or `Vfs::set_owner` (uid/gid
narrowed to 32 bits; the `(uid_t)-1`/`(gid_t)-1` "leave unchanged" sentinels
are honoured by `Vfs::set_owner`). Kernel context keeps the EROFS terminal.
`fchmod`/`fchown` on a non-file fd (pipe/console, no backing inode) return
EINVAL. Regression test: Path Z Part 30 (`self_test_linux_chmod_chown`) — a
hand-built raw-syscall ELF (`build_linux_chmod_chown_test_elf`) calls
`chmod("/chmod-chown-test", 0o640)` then `chown(path, 1234, 5678)` from ring 3
(sentinels `0xE1`/`0xE2`); the harness stages the file on the memfs root and,
after exit 0, independently asserts `Vfs::metadata` reports
`permissions == 0o640`, `uid == 1234`, `gid == 5678`.

**Follow-up (`fchmodat2`, syscall #452):** the 4-arg flags-aware chmod
(`fchmodat2`) was a separate EROFS stub missed by the first pass; it was
wired in the same idiom as `sys_fchownat` during the truncate-line cleanup.
`AT_EMPTY_PATH` resolves `dirfd` to its backing path (AT_FDCWD → cwd, else an
open File fd via `handle_path`); the non-`AT_EMPTY_PATH` branch keeps the
empty-path → ENOENT discrimination then `resolve_at_path` + `chmod_apply`.
Kernel context keeps the EROFS terminal (batch-485 self-test still green).
Regression test: Path Z Part 32 (`self_test_linux_fchmodat2`) —
`build_linux_fchmodat2_emptypath_test_elf` `open(O_RDWR)`s `/fchmodat2-test`
and calls `fchmodat2(fd, "", 0o600, AT_EMPTY_PATH)` (sentinels `0xE5`/`0xE6`);
the harness confirms `Vfs::metadata` reports `permissions == 0o600`.

**Fidelity gaps (minor):**
1. ~~`lchown` and `fchownat(AT_SYMLINK_NOFOLLOW)` must operate on the symlink
   itself, but `Vfs::set_owner` always follows the final symlink.~~
   **RESOLVED 2026-07-22.** Added a no-follow VFS path: `FileSystem`
   trait gained `set_owner_no_follow`/`set_times_no_follow` (default-delegate
   to the following versions — correct for symlink-free FAT), overridden by
   memfs (`resolve_no_follow_mut`) and ext4 (`resolve_path_no_follow`, via
   shared `set_owner_ino`/`set_times_ino` helpers). `Vfs::set_owner_no_follow`
   / `Vfs::set_times_no_follow` wrap them (using `resolve_no_follow` +
   `lmetadata` for the `u32::MAX` "leave unchanged" sentinel). Wired through
   both ABIs: Linux `lchown`/`fchownat(AT_SYMLINK_NOFOLLOW)` →
   `chown_apply_ex(no_follow)`, `utimensat(AT_SYMLINK_NOFOLLOW)` →
   `set_times_no_follow`; native `SYS_FS_SET_OWNER`/`SYS_FS_SET_TIMES` gained
   an `arg4` NO_FOLLOW flag (bit 0), and posix `lchown`/`fchownat`/`utimensat`
   set it. Regression: `fs::handle::self_test` §19 proves no-follow chown
   stamps the link inode while the target owner is untouched, and that a
   following chown hits the target instead. (Symlink-swap-safety analogue of
   the openat2 `RESOLVE_NO_SYMLINKS` fix.)

   **Extended to the xattr family 2026-07-22.** The identical no-follow
   plumbing now backs `lgetxattr`/`lsetxattr`/`llistxattr`/`lremovexattr`,
   which previously followed the trailing symlink and wrongly operated on
   the target. Added `FileSystem::{get,set,remove,list}_xattr[s]_no_follow`
   (default-delegate; memfs overrides via `resolve_no_follow(_mut)` + shared
   `node_*` xattr helpers, ext4 via `resolve_path_no_follow` + shared `*_ino`
   helpers), `Vfs::*_no_follow` wrappers, and a NO_FOLLOW bit in each xattr
   syscall's free arg slot (get/set: `arg5`, list: `arg4`, remove: `arg3`;
   posix threads a `no_follow` bool through `do_getxattr`/`do_setxattr`/
   `do_listxattr`/`do_removexattr`). Regression: `fs::handle::self_test` §20
   proves no-follow xattr set/get/list/remove target the link inode while
   following ops target the file, and the two views never cross. (Closes the
   xattr no-follow limitation formerly tracked in `todo.txt`.)

   **Extended to `chmod` 2026-07-22.** `fchmodat2(AT_SYMLINK_NOFOLLOW)`
   (Linux 6.6+, the very reason the 4-arg syscall exists) previously ignored
   the flag and always followed the trailing symlink (a documented gap in
   `sys_fchmodat2`, "AT_SYMLINK_NOFOLLOW is ignored"). Now plumbed with the
   same no-follow family pattern: `FileSystem::set_permissions_no_follow`
   (default-delegate; memfs overrides via `resolve_no_follow_mut`, ext4 via
   `resolve_path_no_follow` + a new shared `set_permissions_ino` helper),
   `Vfs::set_permissions_no_follow`, a NO_FOLLOW bit in `SYS_FS_SET_PERMS`'s
   free arg slot (`arg3`), and a `chmod_apply_no_follow` in the Linux ABI
   wired from `sys_fchmodat2`'s `AT_SYMLINK_NOFOLLOW`. posix gained `lchmod`
   and routes `fchmodat(AT_SYMLINK_NOFOLLOW)` through it. Regression:
   `fs::handle::self_test` §21 proves no-follow chmod stamps the link inode's
   mode while a following chmod hits the target, and neither crosses.
2. We gate chmod/chown on the generic File-WRITE capability rather than a
   dedicated `CAP_CHOWN`/`CAP_FOWNER`; any process holding File-WRITE can
   change mode/owner. This matches the OS's capability model (no per-syscall
   POSIX capability bits yet) but is laxer than Linux's privilege checks.
