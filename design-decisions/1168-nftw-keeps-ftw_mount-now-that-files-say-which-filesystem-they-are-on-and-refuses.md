## 1168. `nftw` keeps `FTW_MOUNT` now that files say which filesystem they are on -- and refuses it where they do not

**Date:** 2026-10-01
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `nftw`, the C library's "walk a directory tree" call, takes a
flag, `FTW_MOUNT`, that means *do not leave the filesystem you started on* --
what `rm -rf --one-file-system` and `du -x` ask for. It was refused with
"invalid argument" (§761), because no file could say which filesystem it was
on: every `st_dev` was 0. Since 2026-10-01 each mounted filesystem has a
device number of its own, so the walk keeps the flag, as glibc's does. On a
kernel that does not give the number it is still refused, as before.

**What glibc does, measured** (`posix/tools/oracle/ftw_mount_probe.c`, a
tmpfs mounted inside a tree, in a namespace of its own): an entry on another
device than the root's is not reported, and a directory there is not entered
-- the mount point itself is never reported. Without `FTW_PHYS` a symbolic
link is judged by its target, so one pointing across the mount is skipped
too; with it, by the link, which is on the root's filesystem. An entry that
cannot be `stat`ed is `FTW_NS` whatever its device. A root on the mounted
filesystem walks that filesystem. The walk here does all of that, and the
host tests replay the probe's five walks over the same tree.

**Where this departs from glibc:** a root directory whose device is 0 is
`EINVAL`, before any callback. glibc compares whatever `st_dev` says; but on
SlateOS 0 is a kernel that does not say, every file then has it, and a walk
comparing them would cross every mount while reporting success -- the
outcome §761 refused the flag to prevent.

**Alternatives:**

- **Keep refusing it.** §761's reasons were that a comparison written from
  memory, with no filesystem to test it against, is how a plausible but wrong
  implementation gets in. Both are answered: the semantics are measured
  rather than remembered, and the device numbers are real. Refusing now only
  makes the library less useful than glibc's.
- **Ignore a device of 0** (walk as if every file were on the root's
  filesystem). That is accept-and-ignore on an older kernel, the worst of
  §761's three options. Rejected.

This supersedes the `FTW_MOUNT` half of §761; `FTW_CHDIR` is still refused,
for §761's reason.
