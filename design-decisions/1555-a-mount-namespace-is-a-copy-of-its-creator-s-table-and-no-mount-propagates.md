## 1555. A mount namespace is a copy of its creator's table, and no mount propagates

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a program can now give itself a private set of mounts
(`unshare --mount`, `clone(CLONE_NEWNS)`): it starts with a copy of the
mounts it could see, and from then on what it mounts or unmounts is its own,
and what others mount is not seen by it. This is what container tools build
on. Linux also lets mounts be "shared" between such copies, so that a mount
made in one appears in another; that is not built here, and asking for it is
refused rather than quietly ignored. Everything else -- making one, entering
one with `setns`, naming one through `/proc/<pid>/ns/mnt` -- works as on
Linux.

**What exists** (`kernel/src/fs/mntns.rs`, `kernel/src/fs/vfs.rs`):

- The VFS keeps one mount table per mount namespace. The root namespace's is
  the system's; another's is made as a copy of its creator's
  (`Vfs::copy_mount_table`): the same filesystems at the same paths, each
  entry its own (`mnt_id`), so a file held open through one entry counts
  against that entry alone and `umount` in one namespace is refused only for
  what is busy there, as Linux's per-namespace `struct mount` behaves.
- Membership is a process's (design-decisions 1554, decision 1): a map from
  process to namespace behind a leaf lock, asked on every path operation
  only while some namespace besides the root exists; otherwise the answer is
  the root without a lock. Fork and spawn inherit it; the process's end gives
  its hold back; the namespace and its table go with the last hold (a process
  in it, or a `/proc/<pid>/ns/mnt` handle).
- A filesystem mounted only in a namespace that goes is synced and let go
  then, as an unmount's would be; one another table still has stays.
- `setns` into a mount namespace puts the process at `/`, as Linux's
  `mntns_install` does. `CLONE_NEWNS` with `CLONE_FS` is `EINVAL`.

**Decision 1 -- every mount is private.**

| | Private only (chosen) | Propagation (Linux) |
|---|---|---|
| `MS_PRIVATE`, `MS_SLAVE`, `MS_UNBINDABLE` | taken: each is what a mount here already is (a slave of nothing is private) | as Linux |
| `MS_SHARED` | `EINVAL` | mounts in a shared subtree appear in every peer |
| `unshare --mount` (which makes `/` private first) | works | works |
| A host mounting into a running container's view later | not possible | possible with shared/slave mounts (Docker's `rshared` volumes, systemd's mount propagation) |

Propagation needs peer groups and slave relations kept per mount and replayed
on every mount and unmount. Nothing here uses it yet -- the container runtime
isolates through `ipc::namespace` -- and the tools that use mount namespaces
first make `/` private. `EINVAL` for `MS_SHARED` was chosen over accepting it:
a program told its subtree is shared would expect mounts to reach other
namespaces and silently get none. Revisit when a ported program needs it.

**Decision 2 -- the path cache stands aside while more than one table
exists.** The VFS's resolution cache is keyed by path. With a second table,
one path can name two files, and a change made through one namespace's path
cannot invalidate an entry made through another path to the same file. So
while any namespace besides the root exists the cache is neither read nor
filled (`resolution_cacheable`), and it is emptied when the first copy is
made and when the last goes. The common case -- the system's table alone --
keeps the cache. The proper fix keys entries by filesystem and inode rather
than by path (known-issues A-PATH-CACHE-STANDS-ASIDE-WHILE-MOUNT-NAMESPACES-EXIST).

**Decision 3 -- the old `fs::mount_ns` is gone.** It kept per-namespace lists
of mount paths that no path lookup consulted, reachable only from the kernel
shell's `namespace` command. Two modules with the same name and one of them
inert would mislead every reader; the command and `/proc/namespaces` now
report the real namespaces.

**Not yet:** `pivot_root(2)` with Linux's argument rules, stacked mounts and
propagation (known-issues A-LINUX-MOUNT-GAPS; bind and move mounts followed
the same day, design-decisions 1556); the other
namespace kinds (known-issues A-LINUX-NAMESPACE-KINDS-NOT-BUILT-YET).
