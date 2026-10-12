## 1507. A file's device number is a small number per mounted filesystem, reused after unmount, not the mount's permanent id

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** every file reports which disk or filesystem it lives on
(`st_dev`). Tools use it, with the file's inode number, to tell "two names
for one file" from "two files": `tar`, `cp -a`, `du`, `find -samefile`.
Every file here reported 0, so files on different filesystems could be
mistaken for one another. Each mounted filesystem now has a small number of
its own. It is given back when the filesystem is unmounted, and a later
mount may take it, as on Linux.

**Alternatives:**

| | What changes | For | Against |
|---|---|---|---|
| **A. A dense number per live mount, reused (chosen)** | devices count 1, 2, 3 ... as filesystems are mounted at once | small enough for every place a device is reported, the native record's 24 bits included; Linux's own behaviour for anonymous filesystems | a number seen before an unmount can name a different filesystem after; tools compare devices within one walk, so it does not matter there |
| B. The mount's `fs_id` | the permanent id, never reused | no lookup | grows with every mount for the whole uptime; outgrows 24 bits under container churn, and Linux's own 20-bit minor |
| C. A number per filesystem *type* or backing device | e.g. one per disk | stable across remounts | two mounts of one type collide; `tmpfs` has no device at all |

**The encoding:** major 0, the number as minor, built as glibc's
`makedev(0, minor)` (`vfs::linux_dev_t`), so `major()` and `minor()` in a
ported program take it apart.

**Revisit** when real block devices are numbered (a disk partition should
report its block device's numbers, as Linux's ext4 does), or if a program is
found that keeps device numbers across an unmount.
