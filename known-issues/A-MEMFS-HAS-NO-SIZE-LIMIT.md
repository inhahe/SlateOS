### A-MEMFS-HAS-NO-SIZE-LIMIT -- 2026-10-07 -- OPEN (lane A)

**Status:** OPEN (lane A). Found answering
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
