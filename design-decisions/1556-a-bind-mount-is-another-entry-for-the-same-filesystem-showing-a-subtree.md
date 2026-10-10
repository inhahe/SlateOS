## 1556. A bind mount is another entry for the same filesystem, showing a subtree

**Date:** 2026-10-08 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a Linux program running as root can now bind-mount a directory
or a file somewhere else (`mount --bind`), so the same files appear at two
places; make such a bind read-only while the original stays writable; move a
mount (`mount --move`); and mark a mount as one that may not be bound
elsewhere (`--make-unbindable`). Container tools build their volumes this
way: `docker run -v /data:/data` and the bind mounts of `/etc/hosts`,
`/etc/resolv.conf` and `/etc/hostname` into every container. Each bind is
its own mount, as on Linux: removing or renaming its mount point is refused
(`EBUSY`), and moving a file between a bind and its source is refused as a
move between filesystems (`EXDEV`) -- before this change `rmdir` of a mount
point reached the mounted filesystem's root.

**What exists now** (`kernel/src/fs/vfs.rs`, `syscall::linux::sys_mount`):

- `Vfs::bind_mount(source, target, recursive)`: `MS_BIND`, with `MS_REC`
  binding the mounts beneath the source too, unbindable ones and what is
  beneath them left out; a file over a file, a directory over a directory
  (`ENOTDIR` otherwise).
- `Vfs::move_mount(from, to)`: `MS_MOVE`, a mount and every mount beneath it.
- `Vfs::remount_bind`: `MS_REMOUNT | MS_BIND`, one mount's own options.
  `Vfs::remount` (`MS_REMOUNT` alone) changes the mount's options and makes
  the filesystem read-only or writable through every mount of it.
- `Vfs::set_propagation`: `MS_PRIVATE`, `MS_SLAVE`, `MS_UNBINDABLE`, with
  `MS_REC` or not.
- `/proc/<pid>/mountinfo` prints each mount's root (the subtree it shows),
  its own options and the filesystem's `ro`/`rw` separately, and
  `unbindable`.

**Decision 1 -- a bind is a mount-table entry with a root, not a filesystem
that forwards.**

| | An entry sharing the filesystem (chosen) | A forwarding filesystem |
|---|---|---|
| What it is | another `MountPoint` holding the same `Arc` and `fs_id`, and a `root`: the directory of that filesystem it shows | a `FileSystem` whose every call prefixes the path and calls the source's |
| A file through either path | the same file: one `fs_id` and inode, so the same `st_dev`/`st_ino`, locks, page cache and per-file state, as on Linux | a second identity to keep coherent, or every table taught that two `fs_id`s are one |
| Cost | `find_mount` joins the root onto the path below the mount point (nothing for a whole-filesystem mount) | a second lock and a second dispatch on every call through the bind |
| mountinfo's root field | the entry's `root` | invented |

A lookup still walks namespace paths, so `..` at the top of a bind leads to
the directory above its mount point, never above its root in the source, as
Linux's does.

**Decision 2 -- read-only is the mount's or the filesystem's.** Each entry
keeps its own options, and every entry of a filesystem shares one
`fs_read_only` flag (Linux's `SB_RDONLY`; the entry's own is `MNT_READONLY`).
A write is refused when either is set. Without the split, a remount of `/`
read-only would leave every bind of the root filesystem writable, and a bind
made read-only could not be told from a read-only filesystem in mountinfo.

**Decision 3 -- two mounts are two mounts.** A rename, an exchange or a hard
link between two paths is `EXDEV` when they are on different mount-table
entries, not merely different filesystems (it compared filesystem handles
until now, which a bind and its source share). A removal or rename of a name
that is a mount point is `EBUSY`, checked before the intercept, the
auto-version and the quota release run, as Linux's `is_local_mountpoint`.

**Not yet** (known-issues A-LINUX-MOUNT-GAPS): `MS_SHARED`. A mount over a
mount point, which Linux stacks and this table refused (`EBUSY`) -- among
others the recursive bind of a directory onto itself that runc makes before
`pivot_root` -- and `pivot_root(2)` followed the same day (design-decisions
1557).

**Test.** `spawn::self_test_linux_bind_mounts` (`build/bindtest.c`), every
answer checked against Linux 6.6 as root.
