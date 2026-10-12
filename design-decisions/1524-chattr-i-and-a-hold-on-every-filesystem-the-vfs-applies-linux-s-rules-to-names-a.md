## 1524. `chattr +i` and `+a` hold on every filesystem: the VFS applies Linux's rules to names and metadata, the filesystems to contents, and every refusal is `EPERM`

**Date:** 2026-10-02 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** a file marked immutable (`chattr +i`) is meant to be
untouchable -- not written, renamed, deleted, re-moded or linked until root
clears the mark -- and an append-only one (`chattr +a`) only to grow at its
end. Here those marks were honoured piecemeal: on ext4 a rename *onto* an
immutable file replaced it, any `chmod` or `chown` went through, an
append-only file could be deleted; on FAT nothing was refused at all; and
the refusals that did happen said "Permission denied" (`EACCES`) where Linux
says "Operation not permitted" (`EPERM`). Now one module holds Linux's rules
(`fs::attr_policy`), the VFS applies them to every change of a name or of
metadata on every filesystem, the filesystems apply them to every change of
contents, and every refusal is `EPERM`. Lane B's `chattr` and `lsattr` reach
them through `FS_IOC_GETFLAGS`/`FS_IOC_SETFLAGS`.

**The rules** (`fs::attr_policy`, each Linux's):

| Operation | Immutable | Append-only |
|---|---|---|
| write, truncate, whole-file overwrite | refused | only at the end; truncate and overwrite refused |
| open for writing / with `O_TRUNC` | refused | only with `O_APPEND`; `O_TRUNC` refused |
| `fcntl(F_SETFL)` changing `O_APPEND` | -- | refused |
| `fallocate` | refused | allowed |
| unlink, rmdir, rename away, rename onto, link | refused | refused |
| `chmod`, `chown`, times set to given values | refused | refused |
| times set to now (`touch`) | refused | allowed |
| a name added to the directory | refused (immutable directory) | allowed (append-only directory) |
| a name removed from the directory | refused | refused |
| `access(W_OK)` | `EPERM` | -- |

They bind root and kernel tasks too; what only root may do is set or clear
the two marks (Linux's `CAP_LINUX_IMMUTABLE`), and the file's owner may
change any other attribute. Linux's orderings are kept where they are
visible: a missing name is `ENOENT` and a taken one `EEXIST` before `EPERM`,
a chown of (-1, -1) is not refused, a rename of a file onto itself succeeds.

| Where the rules are applied | What changes | For | Against |
|---|---|---|---|
| **Names and metadata in the VFS, under the filesystem's lock; contents in each filesystem, on the inode it writes (chosen -- Linux's split)** | every filesystem that reports the marks is held to them, FAT included, and one added later is too | the data path costs nothing; the check and the change are atomic; one copy of the rules | one or two extra lookups per name or metadata change |
| Each filesystem applies everything | no extra lookups | -- | how ext4 and memfs drifted apart and FAT applied nothing; every new filesystem must re-learn ten rules |
| The VFS applies everything | one place | -- | a metadata lookup per write, and a window between the check and the write |

**FAT's read-only bit** (`fat.rs` `entry_attrs`):

| | What changes | For | Against |
|---|---|---|---|
| **Immutable on a file, nothing on a directory (chosen)** | a read-only file on a stick refuses writes, deletion and renaming; Windows' customised folders take new files | `FileAttr::IMMUTABLE` keeps its one meaning; Windows refuses writing and deleting such a file too, and itself ignores the bit on a folder (`desktop.ini`), as Linux's vfat does unless mounted `rodir` | Windows allows renaming a read-only file, which this refuses |
| Mode bits without `w`, as Linux's vfat | a read-only file is `r--r--r--` | Linux's behaviour | root writes straight through it; FAT's `IMMUTABLE` would mean something else again |

`chattr +a` on FAT, and `+i` on a FAT directory, are refused (`EOPNOTSUPP`)
rather than accepted and ignored, as they were: a mark that reports success
and protects nothing gives a reason to rely on it.

**"Now" is a request** (`fs::vfs::TIME_NOW`, `u64::MAX`): an append-only file
may be touched to now but not set to given times, which could backdate a log,
so the VFS must be able to tell the two apart -- as Linux tells `ATTR_TOUCH`
from `ATTR_TIMES_SET`. The Linux `utime` calls pass `UTIME_NOW` and NULL times
through as `TIME_NOW`; the native `SYS_FS_SET_TIMES` takes it too. Lane D's C
library still reads the clock itself for `UTIME_NOW`
(`requests/a-d-utime-now-is-a-request.md`), so until it passes `TIME_NOW` a
native `touch` of an append-only file is refused.

**Found and fixed on the way:**
- The `FileSystem` trait's default `lmetadata` built a minimal record for
  every name, so on a filesystem that does not override it -- FAT, procfs,
  devfs, the overlay -- `lstat` reported no times, owner, mode or attributes,
  and `ls -l` dated every FAT file 1970. It now gives `metadata` for anything
  but a symlink.
- The Linux `access` consulted no gate and answered every existing file
  writable. It now answers the gates a real open or exec meets: an immutable
  file, the ACLs and capability tags, a read-only mount -- not the mode bits,
  which nothing enforces
  (`TD-B-ACCESS-CANNOT-SEE-THE-ONE-PERMISSION-MECHANISM-THAT-IS-ENFORCED`).
- kshell's `touch` stamped an existing file with the time since boot.

**Not done:** `fs::immutable`, the second, decorative store of the same marks
(known-issues `[A] Two implementations of file immutability`), still exists;
it is the next thing to remove, now that the real one is complete.
