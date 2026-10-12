### A-MEMFS-HAS-NO-SIZE-LIMIT -- 2026-10-07 -- OPEN (lane A)

**Status:** OPEN -- fixed on lane-a-wip 2026-10-09, awaiting a boot on main. Found answering
`requests/d-a-memfs-reports-itself-full.md`.

**In short:** `/tmp` and the other memory file systems (memfs) keep files in
the computer's memory, and nothing stops them from taking all of it. A
program allowed to write `/tmp` can fill memory until other programs, and
the kernel, cannot get any. Linux's equivalent (tmpfs) stops at half the
memory by default and answers "no space left" past that.

**Where:** `kernel/src/fs/memfs.rs`. File contents are `Vec<u8>`s on the
kernel heap, which draws on physical frames; `write`/`truncate` grow them
with no check against any budget. `statvfs` reports the room as held + free
memory (2026-10-07), which is honest about this rather than a fix for it.

**Proper fix:** a size limit per memfs mount, checked where a file grows
(write, truncate, `fallocate`), answering `ENOSPC` past it, reported by
`statvfs` as the total -- tmpfs's `size=`, with tmpfs's default of half the
RAM and an inode limit beside it. **What needs weighing:** the default. Half
the RAM matches Linux, but a large build in `/tmp` on a machine with little
memory then fails where it runs today; a smaller cap protects the system
more and breaks more such builds. A mount option (`size=`) makes either
default adjustable.

**Fix (lane-a-wip, 2026-10-09).** Linux's tmpfs defaults, which settle the
weighing above the way every Linux system already lives with: a memfs holds
at most half of memory (`size`) and half its 4 KiB pages' worth of objects
(`nr_inodes`); past either a write, an overwrite, a truncate or a create is
refused with `DiskFull` (`ENOSPC`). The defaults are read when checked, so a
memfs mounted before memory is counted gets them too. `MemFs` keeps a running
count of the blocks its contents hold (`held_blocks`: each file's bytes,
rounded up to 4 KiB as tmpfs charges pages, and a symlink's target only from
128 bytes up -- tmpfs keeps a shorter one in the inode and charges it nothing,
`SHORT_SYMLINK_LEN`), adjusted wherever contents change or a node goes, so the
check needs no walk; an unlinked file held open keeps its blocks until its
last handle. `statvfs` reports the limits as the totals, as tmpfs does.
(The first boot of the limits found every symlink charged a block, so a
short one was refused on a full memfs that tmpfs would have taken.)

A new tmpfs takes `mount(2)`'s options as Linux's does -- `size=` (with
`k`...`e` suffixes or `%` of memory), `nr_blocks=`, `nr_inodes=`, each `0`
for no limit, and `mode=`, `uid=`, `gid=` for its root -- and refuses one it
does not take with `EINVAL` (`fs::new_filesystem_with_data`). `ramfs`, as
Linux's, has no limit. Not done: changing the limit by remount (Linux allows
`-o remount,size=`).

Tests: `memfs::test_limits` (refusals by each route, room given back,
an open unlinked file's blocks held, the count equal to a walk throughout,
`statvfs`) and `memfs::test_mount_data` (each option, `0` for none, the
defaults, `50%`, six refusals).
