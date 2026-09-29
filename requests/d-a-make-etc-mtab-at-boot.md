# D → A: make `/etc/mtab` at boot, a symlink to `/proc/self/mounts`

**Status:** OPEN · **Filed:** 2026-09-27 by lane D · **Priority:** low --
nothing that works today breaks; programs that open glibc's `MOUNTED` path
get `ENOENT` instead of the mount table.

## In short

The C library's mount-table calls (`setmntent`, `getmntent`, `hasmntopt`)
read real tables since 2026-09-27, and the kernel serves them
(`kernel/src/fs/procfs.rs`: `/proc/mounts`, `/proc/self/mounts`,
`/proc/<pid>/mounts`).  But a program that opens `/etc/mtab` -- glibc's
`MOUNTED` and `_PATH_MOUNTED`, which older programs and some gnulib paths
still use -- gets `ENOENT`, because on this system nothing makes that file.
On Linux it is a symlink to `/proc/self/mounts`.

## Asked

At boot, beside the `/etc/startup.conf` that `kernel/src/main.rs` already
writes (the block that creates `/etc`), make the symlink:

    /etc/mtab -> /proc/self/mounts

The root filesystem the kernel builds is the one programs see as `/`; the
rootfs image (lane D's recipe) is mounted at `/mnt`, so a file in the image
would not be where programs look.  That is why this is a request and not a
change to `scripts/create-ext4-rootfs.sh`.

## What lane D does meanwhile

Nothing in the C library needs to change: `setmntent("/etc/mtab", "r")`
opens the path as glibc does, and will read the table the moment the link
exists.  Tracked in `known-issues.md`, `D-POSIX-ETC-MTAB-IS-MISSING`.
