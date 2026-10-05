### [D] D-POSIX-ETC-MTAB-IS-MISSING — 2026-09-27 — OPEN, fix is lane A's

**Where:** the root filesystem the kernel builds at boot
(`kernel/src/main.rs`, where `/etc` is made); the C library's side is
`posix/src/unistd.rs`'s mntent calls.

**What it is.** A program that opens `/etc/mtab` -- glibc's `MOUNTED` and
`_PATH_MOUNTED` -- gets `ENOENT`.  The mount table is there to read
(`/proc/mounts`, `/proc/self/mounts`, served by `kernel/src/fs/procfs.rs`),
and on Linux `/etc/mtab` is a symlink to `/proc/self/mounts`; here nothing
makes the link.  Programs that name `/proc/mounts` or call `setmntent` on it
are not affected.

**The fix** is one symlink at boot, asked of lane A in
`requests/d-a-make-etc-mtab-at-boot.md`.  Not the rootfs recipe's: the image
is mounted at `/mnt`, not `/`.
