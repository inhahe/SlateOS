## 44. fd-backed VFS ops resolve the path *once* at open() — `*_resolved` worker split (open-fd semantics, double-jail fix)

**Date:** 2026-06-30

**Decided by:** Claude (autonomous). Forced by a correctness bug (fd-backed
file I/O was fundamentally broken for chroot-jailed/container processes); there
is one correct design (an open fd holds a resolved reference), so no operator
fork. CLAUDE.md mandates the proper fix over a band-aid.

**Context — the double-jail bug.** `namespace::apply_root` is intentionally
**non-idempotent**: it blindly prefixes the jail root onto a path, *assuming the
input is a guest (pre-jail) path*. Every path-based `Vfs::*` method begins with
`resolve_follow` → `namespace::resolve_path` → `apply_root`, so a guest path is
jailed exactly once on the way in. But `handle::open()` stored the
*already-resolved host path* in the file handle (`file.path = Vfs::resolve_path(path)`),
and every subsequent fd op (`Vfs::read_at(&file.path)`, `write_at`, `truncate`,
`metadata`, `readdir_at`, `file_identity`, `flock`/`funlock`/`lock_query`) called
`resolve_follow` **again** on that host path → `apply_root` prefixed the jail root
a *second* time → the op hit a path that doesn't exist (`/jail/jail/…`). For a
jailed process even `open()` itself failed, because its internal probes
(`stat`/`truncate`/`write_file`) re-jailed. Non-jailed processes escaped notice
only because `resolve_follow` is idempotent on already-resolved *non-jailed*
paths (apply_root is a no-op when there's no jail). Increment 6's CoW overlay
mount would have exposed this the instant a container opened a file.

**The decision.** Split every path-based `Vfs` method into two functions:
- a thin **wrapper** (`X`) — `let p = Self::resolve_follow(path)?; Self::X_resolved(&p, …)`;
- a **worker** (`X_resolved`) — operates on an already-resolved host path and
  does **no** namespace translation / symlink re-follow.

Handle-backed ops call the `*_resolved` worker directly with the path captured at
`open()`. This encodes correct **open-fd semantics** (Unix): an open file
description is bound to the file it resolved to at open time and is immune to
later chroot, rename, or symlink changes to the path. Split methods: `read_at`,
`read_file`, `stat`, `write_file`, `write_at`, `truncate`, `metadata`,
`read_at_uncached`, `readdir_at`, `file_identity`, `flock`, `funlock`,
`lock_query`. Native path-based syscalls (e.g. `sys_fs_flock`, which takes a raw
user guest path) keep calling the resolving wrapper; only callers holding a
*resolved* path (handle ops, `handle_path()`-derived syscalls) use `*_resolved`.

**Why not option B (store the guest path on the handle, re-resolve each op).**
Re-resolving per op would also fix the double-jail, but it *regresses* fd
stability: a handle would re-follow symlinks and re-resolve renamed/relinked
path components on every read/write, so an fd could silently start pointing at a
different file after a rename or symlink swap — the opposite of Unix open-fd
semantics. Resolving once at open() is both correct and avoids repeating the
(non-trivial) resolution cost on every I/O.

**Regression guard.** `namespace::test_process_root` (run at boot via
`main.rs`) now asserts the non-idempotency directly: resolving an already-jailed
path a second time must produce the double-jailed result. If a future refactor
makes handle ops re-resolve, this boot self-test fails loudly. The existing
`fs::handle::self_test` (open→read→seek→write→read-back→fstat→truncate) provides
end-to-end coverage that the wrapper/worker split itself didn't regress.

**Result.** Build clean, clippy warning count unchanged (17754 before and after
the split — zero net-new), boot-test green. fd-backed file I/O now works for
jailed/container processes.

**Where it bites.** `kernel/src/fs/vfs.rs` (13 method splits + `*_resolved`
workers); `kernel/src/fs/handle.rs` (open/read/write/pread/pwrite/read_dir_at/
metadata/truncate/file_identity/funlock call sites → `*_resolved`);
`kernel/src/syscall/linux.rs` (`sys_flock` → `flock_resolved`/`funlock_resolved`);
`kernel/src/ipc/namespace.rs` (non-idempotency regression assertion). Resolves
the increment-7 double-jail half of TD32; part (b) cwd jailing still open.
