### A-MOUNT-HAS-NO-BIND-OR-MOVE -- 2026-10-08 -- OPEN (lane A)

**Status:** OPEN -- what Linux's `mount(2)` still lacks here, after it was
made to work for root on lane-a-wip 2026-10-08.

**In short:** a Linux program running as root can now mount a filesystem
(`mount -t tmpfs none /mnt/x`), remount one read-only, and unmount one --
normally, lazily (`umount -l`) or on expiry. Until 2026-10-08 every
`mount(2)` and `umount2(2)` was refused, root's included. Several forms
are still refused or ignored, and container tools use some of them.

**Where.** `syscall::linux::sys_mount`, `sys_umount2`; the mount table in
`fs::vfs`; the drivers by type in `fs::new_filesystem`.

| Missing | Answer today | What it needs |
|---|---|---|
| Bind mounts (`MS_BIND`, `mount --bind`), the backbone of container volumes | `EINVAL` | a mount that is a subtree of another filesystem: a `FileSystem` that forwards to the source's with a path prefix, sharing its `fs_id`, so files are the same files |
| Moving a mount (`MS_MOVE`) | `EINVAL` | the mount's and its sub-mounts' paths rewritten under the table's lock, as `Vfs::pivot_mounts` rewrites them |
| `MS_SHARED` (shared propagation) | `EINVAL` | propagation between mount namespaces: no mount here propagates to another, so private, slave and unbindable are taken as what every mount already is |
| A second mount over a mount point (stacking) | `EBUSY` | the table allows one mount per path; Linux stacks them, the last on top |
| `MS_NODEV` | taken, not enforced | device nodes on such a mount refused at open |
| The `data` options (`size=`, `mode=`, `uid=` for tmpfs...) | not read | parsing per type |
| `umount2("/")` | `EBUSY` | Linux remounts the caller's root read-only and answers 0; refusing was judged safer than making the system's root read-only on request |
| `pivot_root(2)` | `EPERM`, root's included | the mount namespace work: `Vfs::pivot_root` exists for the boot and needs Linux's argument rules (`put_old` under `new_root`, `pivot_root(".", ".")`) |

**Test.** `spawn::self_test_linux_mount` (`build/mounttest.c`, checked
against Linux 6.6 as root) covers what works.
