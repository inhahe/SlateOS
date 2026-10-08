### A-LINUX-MOUNT-GAPS -- 2026-10-08 -- OPEN (lane A)

**Status:** OPEN -- what Linux's `mount(2)` still lacks here. Root's
`mount(2)` and `umount2(2)` were made to work on lane-a-wip 2026-10-08; bind
and move mounts (design-decisions 1556), stacked mounts and `pivot_root(2)`
(design-decisions 1557) followed the same day. This entry was
`A-MOUNT-HAS-NO-BIND-OR-MOVE` until then.

**In short:** a Linux program running as root can mount, bind, move, stack
and unmount filesystems, and swap its root with `pivot_root`, as container
tools do. What is left is smaller: shared mounts (a mount made in one place
showing up in another), a few options that are taken and ignored, and some
records that do not follow a `pivot_root` because they are kept by name.

**Where.** `syscall::linux::sys_mount`, `sys_umount2`, `sys_pivot_root`; the
mount table in `fs::vfs`; the drivers by type in `fs::new_filesystem`.

| Missing | Answer today | What it needs |
|---|---|---|
| `MS_SHARED` (shared propagation) | `EINVAL` | propagation between mounts and namespaces (`fs::mntns`, design-decisions 1555): no mount here propagates to another, so private and slave are taken as what every mount already is |
| `MS_NODEV` | taken, not enforced | device nodes on such a mount refused at open |
| The `data` options (`size=`, `mode=`, `uid=` for tmpfs...) | not read | parsing per type |
| `umount2("/")` with nothing stacked on the root | `EBUSY` | Linux remounts the caller's root read-only and answers 0; refusing was judged safer than making the system's root read-only on request |
| Making a mount read-only while a file is open for writing on it | taken; the open file's next write is `EROFS` | Linux refuses the remount (`EBUSY`) while a file on the mount is open for writing: a count of writers per mount |
| An open directory, a watch, a process with a root of its own, across `pivot_root` | kept by path: after the pivot the path names whatever is there now | Linux keeps them as a mount and a directory; here working directories are renamed (design-decisions 1557, decision 5) and the rest is not. A `chroot`ed caller of `pivot_root` is `EINVAL` |
| More than 1024 mounts in one namespace | `ENOSPC` | Linux's limit is 100000 (`fs.mount-max`); `vfs::recompute_visibility` compares every pair of mounts after each change, which wants an index before the limit can rise |

**Test.** `spawn::self_test_linux_mount` (`build/mounttest.c`),
`spawn::self_test_linux_bind_mounts` (`build/bindtest.c`) and
`spawn::self_test_linux_stacked_mounts` (`build/stacktest.c`), all checked
against Linux 6.6 as root, and `fs::vfs::self_test_mount_tree` cover what
works.
