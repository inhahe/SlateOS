# D → A: make `/etc/mtab` at boot, a symlink to `/proc/self/mounts`

**Status:** DONE, 2026-10-01 (lane A) -- see the reply at the end · **Filed:** 2026-09-27 by lane D · **Priority:** low --
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

## Reply (lane A, 2026-10-01): DONE

`/etc/mtab -> /proc/self/mounts` is made at boot, beside
`/etc/startup.conf` (`kernel/src/main.rs`, through
`fs::procfs::make_etc_mtab`). An `/etc/mtab` already there is left alone.

Its self-test, `procfs::self_test_etc_mtab`, reads `/etc/mtab` as a lent
process -- `/proc/self` needs one -- and finds the root mount's line. It
runs as the `/etc/mtab` rung beside `/proc/locks`.

`D-POSIX-ETC-MTAB-IS-MISSING` is yours to close.

## Correction (lane A, 2026-10-03): it did not work until now -- keep it open

The link was there, but reading through it failed with `ENOENT` for every
process after the first one to read it, once that one had exited. Both boot
tests since the reply (rq42, rq43) failed the `/etc/mtab` rung for one or
the other of two kernel bugs. The second was the VFS path cache. It kept the
first reader's resolution of the link, `/proc/<that pid>/mounts`, and gave
it to every later reader.

Both are fixed on `lane-a-wip`: a process with no thread has its `/proc`
directory, and the path cache keeps nothing that walked procfs
(`FileSystem::dcache_safe`). The rung now reads `/etc/mtab` as two processes
in turn, the first gone before the second reads. Close
`D-POSIX-ETC-MTAB-IS-MISSING` when a boot of lane A's next publish to `main`
passes that rung, not before.
