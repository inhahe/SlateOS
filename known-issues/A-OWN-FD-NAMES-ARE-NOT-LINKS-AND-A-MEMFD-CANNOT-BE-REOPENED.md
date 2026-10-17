### A-OWN-FD-NAMES-ARE-NOT-LINKS-AND-A-MEMFD-CANNOT-BE-REOPENED -- 2026-10-07 -- OPEN (lane A)

**Status:** OPEN (lane A) -- two gaps left by the Linux-ABI reopen of
`/dev/stdin`, `/dev/fd/N` and `/proc/self/fd/N` (lane B's request
`b-a-dev-stdin-stdout-and-stderr-are-console-nodes-...`, done 2026-10-07).
**The memfd half is fixed** (2026-10-17, on lane-a-wip): a memfd is a file
now (design-decisions §1573), and reopens as one. Only the links remain.

**In short:** a Linux program can now open `/dev/stdin` and the like and
get what its own descriptor holds, and `stat` reports that object. Two things
still differ from Linux: asking about the *name itself* (`lstat`,
`readlink` of `/dev/stdin`) still describes a console device rather than a
link, and opening one of these names for a descriptor that holds a memfd
answers "no such device" where Linux opens the memfd again.

**Where:**

- `lstat` / `readlink` / `newfstatat(AT_SYMLINK_NOFOLLOW)` of `/dev/stdin`,
  `/dev/stdout`, `/dev/stderr`: devfs's `chr_served` nodes
  (`kernel/src/fs/devfs.rs`). Linux has symbolic links there (`lstat` mode
  `S_IFLNK|0777`, `readlink` `/proc/self/fd/0`). `/dev/fd` does not exist in
  devfs at all (Linux: a link to `/proc/self/fd`), though opening a name
  under it works (`syscall::linux::own_fd_name` answers before the VFS).
- ~~A memfd: `syscall::linux::reopen_own_fd` answered `ENXIO`.~~ Fixed
  2026-10-17: a memfd is an unnamed file on the kernel's own memfs
  (§1573), so `reopen_own_fd` makes a new description of it as of any file
  (`fs::handle::reopen`), at offset 0 -- how a program gets a read-only
  descriptor of a sealed memfd. The shared-mapping fixture's step 17 reopens
  one read-only through `/proc/self/fd/N`, reads its bytes and is refused a
  write, as on Linux 6.6.

**Proper fix:** devfs nodes of kind link for the three names and `/dev/fd`,
whose `readlink` answers `/proc/self/fd/N` (and `/proc/self/fd`), with the
open-time and stat-time interception kept as the following of those links;
(The memfd half's fix, a description separate from the object, came with
the memfd becoming a file.)

**Native programs** are unaffected: lane D's C library answers all these
names itself, links and all (`posix/src/fdname.rs`).
