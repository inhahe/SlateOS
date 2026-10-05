### B-TRUNC1. Linux-ABI `truncate`/`ftruncate` returned EROFS unconditionally for confirmed regular files (stale FS-mutation stubs) — FIXED 2026-06-16

**Symptom:** No ring-3 program could resize a file. Both `truncate(2)` and
`ftruncate(2)` ran their full input-shape gate ladders (EINVAL on negative
length, EFAULT/ENOENT on bad/empty/missing paths, EISDIR on directories,
EINVAL on non-regular inodes / read-only-fd via the FMODE_WRITE check) and
then hard-coded `linux_err(errno::EROFS)` for a confirmed regular file —
even though the VFS exposes `Vfs::truncate` implemented across
memfs/ext4/fat. Every database that pre-sizes its file (sqlite, lmdb),
log rotators, `dd`/`truncate(1)`, `fallocate`-fallback paths, and `./configure`'s
`AC_FUNC_FTRUNCATE` probe depend on this.

**Root cause:** Placeholder terminals from the universal-read-only era.
`sys_truncate` already did a real `Vfs::stat` triage (so the EISDIR/EINVAL/
ENOENT diagnostics were live), and `sys_ftruncate` already enforced the
`FMODE_WRITE` gate; only the final File-arm answer was a hard-coded EROFS.
Both terminals are asserted in kernel context by the batch-447/448 fidelity
self-tests, which short-circuit on `caller_pid().is_none()`.

**Fix (proper):** For ring-3 callers `sys_truncate` (after the stat triage
confirms a regular file) and `sys_ftruncate`'s File arm now enforce
`RLIMIT_FSIZE` (EFBIG on a grow past the soft limit — the check returns to
its proper Linux position now that mounts are writable, between the
`FMODE_WRITE`/`mnt_want_write` gate and `do_truncate`), require a File-WRITE
capability, resolve the target (`canonicalize_path` for the path variant,
`handle_path` on the open fd for `ftruncate`), and call `Vfs::truncate`,
which grows-with-zeros or shrinks the file. Kernel context keeps the EROFS
terminal (gated on `caller_pid().is_none()`). Regression test: Path Z Part 31
(`self_test_linux_truncate`) — a hand-built raw-syscall ELF
(`build_linux_truncate_test_elf`) shrinks `/truncate-test` to 4 bytes via the
path syscall, then `open(O_RDWR)` + `ftruncate(fd, 10)` grows it; sentinels
`0xF1`/`0xF2`/`0xF3`. The harness stages a 16-byte file on the memfs root and,
after exit 0, independently asserts the readback is exactly 10 bytes with the
leading 4 preserved (`'A'`) and the grown tail zero-filled.

**Fidelity gaps (minor):**
1. `Vfs::truncate` follows the final symlink (`resolve_follow`), matching
   Linux's `truncate(2)` (which also follows). `ftruncate` operates on the
   fd's already-resolved inode, also correct. No no-follow truncate exists
   in Linux, so there is no gap here — noted only for symmetry with B-CHOWN1.
2. We gate the resize on the generic File-WRITE capability; Linux additionally
   honours `IS_APPEND`/`IS_IMMUTABLE` inode flags (EPERM) and the per-fd
   `O_APPEND`-doesn't-block-truncate nuance. The append/immutable-flag EPERM
   path is not yet plumbed (same capability-model gap as B-CHOWN1).
