### B-SYM1. Linux-ABI `symlink`/`symlinkat` returned EROFS and `readlink`/`readlinkat` returned EINVAL unconditionally (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could create or resolve a symbolic link. Every
`symlink(2)`/`symlinkat(2)` returned `-EROFS` ("read-only file system") and
every `readlink(2)`/`readlinkat(2)` returned `-EINVAL` ("not a symlink"),
regardless of whether the path existed or was actually a symlink, even though
the VFS is fully writable and natively supports symlinks (`Vfs::symlink`,
`Vfs::readlink`). This breaks any toolchain that relies on symlinks (build
systems, `ld` SONAME links, package layouts).

**Root cause:** The four handlers were placeholder stubs left over from before
the VFS gained symlink support: `sys_symlink`/`sys_symlinkat` validated their
path arguments and then hard-coded `linux_err(errno::EROFS)`, and
`sys_readlink`/`sys_readlinkat` hard-coded `linux_err(errno::EINVAL)` after
the argument gates. The stubs' errno terminals were also (correctly) asserted
by the batch-478/487 syscall-fidelity self-tests, which call the handlers in
kernel context with fake pointers — so the fix had to keep those terminals for
kernel callers while doing real work for ring-3 callers.

**Fix (proper):** Wire all four to the VFS for ring-3 callers.
`sys_symlink`/`sys_symlinkat` share a new `symlink_common(target_ptr,
newdirfd, linkpath_ptr)` that stores the `target` *verbatim* (a symlink may
dangle and may be relative — it is NOT resolved or canonicalised), resolves
the `linkpath` against the caller's cwd / `newdirfd` via `resolve_at_path`,
requires a File-WRITE capability (`require_fs_write`), and calls
`Vfs::symlink`. `sys_readlink`/`sys_readlinkat` share a new
`do_readlink_copy(path, buf_ptr, bufsiz)` that calls `Vfs::readlink` and
copies `min(target_len, bufsiz)` bytes with **no** trailing NUL, returning the
byte count (the Linux `do_readlinkat` contract); the user buffer is validated
and written only *after* the dentry is confirmed to be a symlink
(`NotFound`→ENOENT, `InvalidArgument`→EINVAL/"not a symlink"). Kernel context
(`caller_pid().is_none()`) preserves the prior EROFS/EINVAL terminals;
`sys_readlink` canonicalises the path against `pcb::get_cwd` first, the `*at`
variants use `resolve_at_path`. Regression test: Path Z Part 27
(`self_test_linux_symlink_readlink`) — a hand-built raw-syscall Linux-ABI ELF
(`build_linux_symlink_readlink_test_elf`) calls `symlink("Z", "/sl-rl-link")`
then `readlink("/sl-rl-link", buf, 64)` from ring 3 and asserts the call
returned exactly 1 byte == `'Z'` (self-diagnosing exit sentinels
`0xB1`/`0xB3`/`0xB4`). The harness pre-removes the link path and, after the
process exits 0, independently confirms kernel-side via `Vfs::readlink` that
the created link resolves to `"Z"`. (Raw ELF rather than dash because dash has
no `ln` builtin and cannot invoke `symlink(2)`/`readlink(2)` directly.)
**Follow-up — `link`/`linkat` now wired (2026-06-16):** `sys_link`/`sys_linkat`
share a new `link_common` that resolves both names via `resolve_at_path`,
requires a File-WRITE capability, and calls `Vfs::link` (kernel context still
EROFS). Regression test: Path Z Part 28 (`self_test_linux_link`) hard-links
`/mnt/lnk-dst` to a pre-staged `/mnt/lnk-src` from ring 3 and reads the byte
back through it.

**memfs does not support hard links (deferred):** *— no longer true as of
2026-09-01; memfs was refactored to a flat inode table and implements
`link`/`link_no_follow`. See `design-decisions.md` §665 and
`A-MEMFS-CANNOT-HARD-LINK-SO-NO-BOOT-EVER-TESTS-A-HARD-LINK`. The paragraph is
kept because the fidelity-gap history after it is still accurate.* The test
runs on the **ext4**
mount at `/mnt`, not the in-memory root (`/`, `/tmp`). memfs stores file data
inline in by-value tree nodes (`MemFsNodeKind::File(Vec<u8>)` owned by the
parent's `BTreeMap`), so two directory entries cannot share one inode — which
is exactly what a hard link requires. memfs therefore correctly returns
"unsupported" (Linux returns **EPERM** for filesystems without hard-link
support). Proper fix: refactor memfs to an inode-table model (`MemFs` owns
`BTreeMap<ino, Inode>`; file/symlink directory entries hold an `ino` instead of
the body, so multiple names can reference one inode with a shared `nlink`).
This is a sizeable refactor of a core subsystem with many passing self-tests,
and ext4 (the design's real root FS) already implements hard links, so it is
deferred rather than done speculatively. **Fidelity gap (RESOLVED 2026-07-22):**
`Vfs::link` used to always follow a symlink `oldpath`, whereas plain `link(2)`
must NOT follow (hard-link the symlink inode itself) and `linkat` follows only
with `AT_SYMLINK_FOLLOW`. Fixed with the same no-follow VFS plumbing as the
chown/times/xattr family: `FileSystem::link_no_follow` (default-delegates to
`link` — correct for FAT/memfs, which lack hard links / symlinks; ext4 overrides
it via `resolve_path_no_follow` + a shared `link_ino_checked` helper that now
also accepts `S_IFLNK`), and `Vfs::link_no_follow` (a thin wrapper over the new
`link_inner(follow)` back-end that resolves `oldpath` with `resolve_no_follow`).
Wiring: native `SYS_FS_LINK` gained an `arg4` bit-0 FOLLOW flag (default clear =
no-follow = `link(2)`); Linux-ABI `link_common` takes a `follow` bool
(`sys_link` → false, `sys_linkat` → `flags & AT_SYMLINK_FOLLOW`); posix `link`
stays no-follow (syscall4→syscall5 with follow=false) and `linkat` routes
`AT_SYMLINK_FOLLOW` through a shared `link_ex`, and now validates its flags
(`~(AT_SYMLINK_FOLLOW|AT_EMPTY_PATH)` → EINVAL). Regression:
`proc::spawn::self_test_ext4_link_no_follow` (kernel context, ext4 `/mnt`)
proves no-follow hard-links the symlink inode (the new name is itself a symlink)
while a following link hard-links the target file.

**Bug found & fixed while landing the above (ext4 `unlink` followed the final
symlink, 2026-07-22):** `Ext4Fs::remove` resolved its path with
`driver.resolve_path` (FOLLOW), so `unlink(2)`/`remove` on a symlink dereferenced
it — decrementing the *target's* `i_links_count` while deleting the *symlink's*
directory entry (data corruption when the target exists), and failing outright
(`NotFound`) when the target was unreachable (e.g. a symlink whose stored target
is an absolute VFS path on another mount, which ext4 mis-resolves relative to its
own root). `unlink(2)` must never dereference the final symlink. Fixed to
`resolve_path_no_follow` (parent still resolved with follow, which is correct).
memfs's `remove` was already correct (operates on the entry itself). This bug
had previously created a stray directory entry `/nfl-link` on the persistent
`rootfs.ext4` test image that hard-linked the ext4 **root inode** (a
directory-typed dirent → inode 2), left by a pre-`link_ino_checked`-guard run;
the current `link_ino_checked` rejects directory hard-links (`EISDIR`) so it
cannot recur. The stale entry was removed surgically with
`debugfs -w -R "unlink /nfl-link"` + `e2fsck -fy` (which also re-homed an
orphaned stale `nfl-target` inode to `lost+found` and corrected root's inflated
link count). The self-test's `drain` now removes fixtures via `Vfs::remove`
(no-follow) in a loop rather than an `exists` guard (which follows symlinks and
would skip a dangling symlink).

**Same bug class in `rmdir`/`rename` — fixed (2026-07-22):** `Ext4Fs::rmdir` and
`Ext4Fs::rename` had the identical defect: both re-resolved their final operands
with `driver.resolve_path` (FOLLOW) even though the VFS layer already resolves
no-follow. Per POSIX neither call dereferences a trailing symlink: `rmdir(2)` on
a symlink must return `ENOTDIR` (the symlink itself is not a directory, and
following it would let it destroy the *target* directory while unlinking the
symlink's name), and `rename(2)` renames the *symlink itself* on either operand
(and replaces an existing symlink destination, not its target) — following would
rename/replace the wrong inode and mis-stamp the re-inserted dirent's type byte.
Both were changed to `resolve_path_no_follow` for the final components (parents
still resolved with follow — the path *to* a parent correctly follows
intermediate symlinks). The ext4 no-follow self-test
(`self_test_ext4_link_no_follow`) was extended with regression steps: (c)
`rmdir(symlink)` must return `NotADirectory` and leave the link intact, and (d)
`rename(symlink)` must preserve the link (new name still a symlink to the target)
and leave no old name behind. Boot-verified green:
`link()/linkat no-follow: OK (... rmdir/rename honour no-follow)`.

**Follow-up — `utimensat`/`utimes`/`utime` now wired (2026-06-16):** see
B-UTIME1 below.
