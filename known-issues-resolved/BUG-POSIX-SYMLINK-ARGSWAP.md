### BUG-POSIX-SYMLINK-ARGSWAP. posix `symlink()` libc wrapper passed `SYS_FS_SYMLINK` args swapped vs. the kernel ABI → symlink-to-existing-target failed EEXIST — FIXED 2026-07-22

**Where:** `posix/src/file.rs`, `symlink(target, linkpath)`.

**Bug:** the kernel `sys_fs_symlink` (`kernel/src/syscall/handlers.rs`) documents
and reads its arguments as `arg0/arg1 = link path`, `arg2/arg3 = target`, calling
`Vfs::symlink(link_path, target)` (create a link *at* link_path pointing *to*
target). The posix libc wrapper instead issued `syscall4(SYS_FS_SYMLINK, target,
target_len, link_resolved, link_len)` — i.e. it put the **target** in arg0 and the
**link path** in arg2, exactly reversed. Consequence: the kernel tried to create
the symlink *at the target path*. When the target already existed (the normal
case — you symlink to a file that's there), the create failed with `EEXIST`, so
`symlink()` returned -1. If the target didn't exist, it would instead create a
link with swapped semantics (link created at the target name, pointing at the
link name).

**How it was caught:** the new `fastpy-symlink` ring-3 self-test
(`self_test_fastpy_slateos_symlink`, `kernel/src/proc/spawn.rs`) is
false-pass-proof — it stages the target file first, then runs
`os.symlink(target, link)` and both (a) checks the tool's exit code (0 only on a
readback match) and (b) independently re-reads the link via `Vfs::readlink`. The
tool exited 3 (`os.symlink` failed), pinpointing the wrapper.

**Fix:** reordered the `syscall4` arguments to match the ABI — link path in
arg0/arg1, target in arg2/arg3. Required rebuilding the posix sysroot `libc.a`
(the fix lives in the statically-linked libc, not the kernel) and relinking the
fastpy ELF. Boot-confirmed: the round-trip target match now verifies in-process
and is re-verified by the kernel VFS. (`readlink()` in the same file was already
correct — only `symlink()` was swapped.)
