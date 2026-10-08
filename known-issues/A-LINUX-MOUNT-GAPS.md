### A-LINUX-MOUNT-GAPS -- 2026-10-08 -- OPEN (lane A)

**Status:** OPEN -- what Linux's `mount(2)` still lacks here. Root's
`mount(2)` and `umount2(2)` were made to work on lane-a-wip 2026-10-08, and
bind and move mounts followed the same day (design-decisions 1556). This
entry was `A-MOUNT-HAS-NO-BIND-OR-MOVE` until then.

**In short:** a Linux program running as root can mount a filesystem, bind a
directory or a file elsewhere (`mount --bind`, with `--rbind` too), make a
bind read-only, move a mount, make one unbindable, and unmount -- normally,
lazily (`umount -l`) or on expiry. Two things container tools do are still
refused: mounting something on a place that is already a mount point (Linux
puts the new mount on top), and `pivot_root`, which swaps a container's root
for the real one. A few smaller options are taken and ignored.

**Where.** `syscall::linux::sys_mount`, `sys_umount2`, `sys_pivot_root`; the
mount table in `fs::vfs`; the drivers by type in `fs::new_filesystem`.

| Missing | Answer today | What it needs |
|---|---|---|
| A mount over a mount point (stacking), by a new mount, a bind or a move | `EBUSY` | the table allows one mount per path; Linux stacks them, the last on top, and a mount over a directory hides the mounts beneath it. runc's recursive bind of its root directory onto itself is one: the copies land on the paths of the mounts they copy |
| `pivot_root(2)` | `EPERM`, root's included | Linux's argument rules over the caller's mount table (`Vfs::pivot_mounts` already works on it, for the boot): `put_old` at or under `new_root`, the old root's mounts moving under `put_old` with it, and `pivot_root(".", ".")`, which runc, bubblewrap and LXC use |
| `MS_SHARED` (shared propagation) | `EINVAL` | propagation between mounts and namespaces (`fs::mntns`, design-decisions 1555): no mount here propagates to another, so private and slave are taken as what every mount already is |
| `MS_NODEV` | taken, not enforced | device nodes on such a mount refused at open |
| The `data` options (`size=`, `mode=`, `uid=` for tmpfs...) | not read | parsing per type |
| `umount2("/")` | `EBUSY` | Linux remounts the caller's root read-only and answers 0; refusing was judged safer than making the system's root read-only on request |
| Making a mount read-only while a file is open for writing on it | taken; the open file's next write is `EROFS` | Linux refuses the remount (`EBUSY`) while a file on the mount is open for writing: a count of writers per mount |

**Test.** `spawn::self_test_linux_mount` (`build/mounttest.c`) and
`spawn::self_test_linux_bind_mounts` (`build/bindtest.c`), both checked
against Linux 6.6 as root, cover what works.
