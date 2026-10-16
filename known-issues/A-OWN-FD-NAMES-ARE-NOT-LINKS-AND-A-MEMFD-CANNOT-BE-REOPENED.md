### A-OWN-FD-NAMES-ARE-NOT-LINKS-AND-A-MEMFD-CANNOT-BE-REOPENED -- 2026-10-07 -- OPEN (lane A)

**Status:** OPEN (lane A) -- two gaps left by the Linux-ABI reopen of
`/dev/stdin`, `/dev/fd/N` and `/proc/self/fd/N` (lane B's request
`b-a-dev-stdin-stdout-and-stderr-are-console-nodes-...`, done 2026-10-07).

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
- A memfd: `syscall::linux::reopen_own_fd` answers `ENXIO`. Linux re-opens
  it (a memfd is a tmpfs file): a new description at offset 0, which is how
  a program gets a read-only descriptor of a sealed memfd. Here a memfd's
  offset lives in its one handle (`ipc::memfd`), so a second description of
  the same memfd cannot be made.

**Proper fix:** devfs nodes of kind link for the three names and `/dev/fd`,
whose `readlink` answers `/proc/self/fd/N` (and `/proc/self/fd`), with the
open-time and stat-time interception kept as the following of those links;
and a memfd description separate from the memfd object (offset and flags per
description, the object refcounted), which `memfd::reopen` then makes.

**Native programs** are unaffected: lane D's C library answers all these
names itself, links and all (`posix/src/fdname.rs`).
