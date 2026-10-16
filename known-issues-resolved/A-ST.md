### A-ST_DEV-IS-ZERO-FOR-EVERY-FILE -- 2026-10-01 -- FIXED the same day (lane A; design-decisions §1507)

**In short:** every file on every filesystem reports device number 0. Tools
recognise "the same file" by the pair (device, inode), so two different
files on two filesystems that happen to share an inode number look like one
file. That affects `tar` and `rsync -H`, which preserve hard links (a false
one corrupts the archive), `cp -a`, `du` (counts a "hard link" once) and
`find -samefile`. It also blinds `find -xdev` and `du -x`, which stay on
one filesystem by comparing devices.

**Where:** `syscall/linux.rs`'s `stat` and `statx` fills write `st_dev = 0`
(`put_u64(buf, 0, 0); // st_dev`), and `FileMeta` carries no device at
all. The VFS knows the answer, `FileId::fs_id`: each mount's stable id,
already the identity key of the page cache and the lock tables.

**Found** while giving `/proc/locks` Linux's `major:minor:inode` field.
That field now prints the filesystem id under major 0, as Linux numbers its
anonymous filesystems, which matches `stat` once `stat` reports it.

**Fixed, 2026-10-01:**
- **A device number per live mount** (`vfs::dev_of`). It is small,
  assigned at mount and reused after unmount, as Linux reuses an anonymous
  device's minor. It is not `fs_id`, which must never be reused because it
  keys the page cache and the lock tables.
- **`FileMeta::dev`**, filled by the VFS from the mount a file was found
  on.
- **Who reports it:**
  - Linux `stat` (`st_dev = makedev(0, dev)`) and `statx`
    (`stx_dev_major`/`stx_dev_minor`);
  - the native stat record, in its three reserved bytes `[9..12]`. libc
    still has to read them: lane D, by message;
  - `/proc/locks`;
  - `/proc/<pid>/mountinfo`'s `major:minor`, which was the mount's index
    and moved whenever an earlier mount went;
  - `/proc/<pid>/maps` for a file-backed region, now with its offset,
    inode and path as well.
- **Tests:** `vfs`'s `device_numbers_self_test` (a scratch mount's files
  report a device of their own, the number is reused, and the `dev_t`
  survives glibc's `minor()`), and the `mountinfo` and `maps` render
  tests.

**Found doing it:** `A-LINUX-FSTAT-OF-A-FILE-WAS-MADE-UP`, the next entry.
