## 1109. `ftw` walks on one descriptor, and an inode of 0 is not an identity

**Date:** 2026-09-26
**Lane:** D
**Decided by:** Claude (autonomous)

**In short:** `ftw` and `nftw` walk a directory tree, calling a function for
every file. They used to stop a few levels down — as many levels as the
program allowed open directories, never more than 32 — and report everything
deeper as unreadable (`known-issues.md` → `B-D-FTW-STOPS-AT-NOPENFD-DEEP`).
They are now glibc 2.39's walker (io/ftw.c), with two choices of our own.

**1. Each level's names are read, and its directory closed, before the walk
descends.** glibc keeps up to `nopenfd` directories open and, when it needs
one more, reads the rest of the oldest into memory and closes it. Our
directory stream is already a snapshot taken when it is opened, so reading it
out costs nothing that keeping it open would not.

| Option | For | Against |
|---|---|---|
| (a) **read every level eagerly (chosen)** | one descriptor at a time, at any depth; no bookkeeping of which stream to evict | glibc `stat`s relative to the open parent, so it has no path-length limit; ours `stat`s by full path and stops at 4096 bytes — which it did before, too |
| (b) glibc's lazy eviction | the same descriptors glibc would hold | a ring of streams to manage for no gain while `stat` goes by path |

**2. A directory whose inode is 0 is not recorded as visited.** glibc records
every directory it enters by `(st_dev, st_ino)` and silently skips one it has
entered, which is what stops a symbolic-link loop. Linux never reports inode
0; this system does, for filesystems with no stable identity (§740: procfs,
sysfs, devfs, iso9660, FAT).

| Option | For | Against |
|---|---|---|
| (a) **don't record inode 0 (chosen)** | every directory on such a filesystem is walked | a link loop through them ends at the path limit with `ENAMETOOLONG` instead of being skipped |
| (b) record it as glibc would | loops always stopped | every inode-0 directory after the first on a device is skipped without a word — a walk of `/proc` would see one directory |
