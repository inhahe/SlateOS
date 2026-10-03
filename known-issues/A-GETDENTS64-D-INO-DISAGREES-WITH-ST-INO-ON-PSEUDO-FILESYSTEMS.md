## A-GETDENTS64-D-INO-DISAGREES-WITH-ST-INO-ON-PSEUDO-FILESYSTEMS (lane A, 2026-08-31)

**What:** On the four filesystems that have no per-object identity —
`procfs`, `sysfs`, `devfs`, `iso9660`, plus FAT files with no allocated
cluster — the Linux-ABI `getdents64` reports a `d_ino` that a `stat` of the
same name will not confirm. `stat` answers 0 (`FileMeta::minimal`'s `ino`,
which those filesystems never override); `getdents64` answers
`synth_inode`, the FNV-1a hash of the path.

**Where:** `kernel/src/syscall/linux.rs`, the `d_ino` selection in
`sys_getdents64` and `synth_inode` below it; `fill_stat_from_meta`'s
`put_u64(buf, 8, meta.ino)` is the other half.

**Why it is here and not fixed:** this is the deliberate residual of §662,
not an oversight. Making the two agree means emitting a literal `0` for
`d_ino`, and a `getdents64` caller is entitled to read `d_ino == 0` as a
deleted entry and skip the name — which would drop entries from
`readdir("/proc")` and `readdir("/dev")` outright. Losing a name is worse
than reporting an identity a stat will not confirm, so the hash stays. Every
filesystem with a real inode (ext4, btrfs, f2fs, zfs, ntfs, memfs, and FAT
files with a cluster) now agrees exactly, which is where the cross-checks in
`tar`, `rsync`, `du`, `cp` and `find -inum` actually bite.

**Reproduce:** boot, then compare `ls -i /proc/self` against
`stat -c %i /proc/self/<name>` for any name in it. The first prints a large
hash, the second prints 0.

**The proper fix, when it is worth doing:** give the pseudo-filesystems real
inode numbers. `procfs` and `sysfs` generate their trees from static tables
(`SYS_DIRS`/`SYS_FILES` and the sysfs equivalents), so a number derived from
the table index plus the PID for per-process nodes would be stable, unique,
and reportable from *both* `metadata()` and `readdir()` — which is the actual
requirement. `devfs` already has a node registry to key off. That closes the
gap properly rather than choosing which of two wrong answers to give, and it
is the same shape as the fix for the `nlinks` and `permissions` those
filesystems also report as placeholders. Until then the disagreement is
documented rather than silent, which is the difference that matters.
