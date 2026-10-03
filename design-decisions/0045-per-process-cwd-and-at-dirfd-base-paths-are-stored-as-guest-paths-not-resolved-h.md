## 45. Per-process cwd and `*at` dirfd base paths are stored as *guest* paths, not resolved host paths (chroot relative-path containment)

**Date:** 2026-06-30

**Decided by:** Claude (autonomous). Closes the relative-path-containment half of
TD32 part (b); the guest-path representation is the obviously-correct choice
(consistency with how `chdir` and the canonicalize-then-jail pipeline already
work), not an operator fork.

**Context.** Relative paths are canonicalized against the per-process cwd in the
*syscall* layer (`open_common`, `resolve_at_path`) → an absolute path → then the
VFS jails it via `apply_root`. So a relative path is contained **iff the cwd it
joins against is a guest path** (jailed exactly once on the way out). `chdir`
already stored a guest cwd. But three sites stored/used the *resolved host* path:
- `fchdir` stored `handle_path(fd)` (the resolved host path) as cwd → `getcwd`
  leaked the jail's host location, and a later relative path joined the host cwd
  and was jailed a *second* time (double-jail → nonexistent path).
- `sys_openat(realdirfd, rel)` built `host_dir + "/" + rel` and re-opened it
  (re-jailed), and its directory type-check `Vfs::stat(&host_dir)` re-jailed too
  (→ ENOENT for *every* relative `*at` from a jailed process).
- `resolve_at_path` (shared resolver for fstatat/unlinkat/fchownat/…) had the
  identical defect.

These didn't bite the common container launch (image entrypoints + libs use
absolute paths), but any container process using `fchdir`/relative `*at` would
break, and `getcwd` leaked the host jail path.

**The decision.** Represent **all** stored/derived directory bases as *guest*
paths, so the single canonicalize-then-`apply_root` pipeline jails them exactly
once. Concretely:
- Added `namespace::unjail_path_for(pid, host) → guest` — the exact inverse of
  `apply_root`: strip the process's jail-root prefix (no-op for an unjailed
  process; `host == root` → `/`; out-of-jail host returned unchanged
  defensively).
- `fchdir` converts `handle_path` (host) back to guest with `unjail_path_for`
  before `set_cwd`.
- New shared helper `dirfd_to_guest_dir(dirfd)` resolves a real dirfd to its
  *guest* directory path, doing the directory-type check with `stat_resolved`
  (the §44 worker — no re-jail). Both `sys_openat` and `resolve_at_path` use it,
  replacing their duplicated host-path-prepend logic.

**Why guest paths, not "store the host cwd and skip re-jailing"** (the
alternative the original TD32 note sketched). If cwd were stored as a host path,
the canonicalizer would produce a host absolute path, but it cannot distinguish
that from a genuine *guest* absolute path the user passed (`open("/etc/x")`),
which **must** be jailed. One uniform rule — "everything entering the VFS is a
guest path, jailed once" — is only possible if cwd is a guest path. This also
keeps `chdir` and `fchdir` representations consistent (both guest), so
`get_cwd`/`getcwd` always returns a guest path.

**Why not store the guest path on the open handle** (which would make
`unjail_path_for` unnecessary). That is the fully general solution and the only
way to also reverse namespace Bind/Hide remapping, but it enlarges every
`OpenFile` and the open path for a case that does not occur: the container
runtime isolates with the chroot jail alone and never layers Bind rules on a
jailed process, so stripping the chroot prefix is exact. The limitation (a
Bind-rules-*and*-chroot process that `fchdir`s would get the post-Bind guest
path) is documented on `unjail_path_for`; revisit only if such combos arise.

**Regression guard.** `namespace::test_process_root` (boot self-test) now asserts
the round trip: `unjail_path_for(pid, resolve_path_for(pid, g)) ==` the
normalized guest path, the unjailed no-op, and the out-of-jail passthrough.

**Result.** Build clean; warning counts for every touched file unchanged vs the
prior commit (linux.rs 2341, vfs.rs 69, namespace.rs 8 — zero net-new); boot-test
green. Closes TD32 part (b); TD32's remaining scope is the larger
mount-namespace/`pivot_root` feature deferred in §42.

**Where it bites.** `kernel/src/ipc/namespace.rs` (`unjail_path_for` +
round-trip assertions); `kernel/src/syscall/linux.rs` (`dirfd_to_guest_dir`
helper, `sys_fchdir`, `sys_openat`, `resolve_at_path`).
