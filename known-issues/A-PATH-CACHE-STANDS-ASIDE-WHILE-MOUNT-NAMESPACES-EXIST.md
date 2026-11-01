### A-PATH-CACHE-STANDS-ASIDE-WHILE-MOUNT-NAMESPACES-EXIST -- 2026-10-08 -- OPEN (lane A)

**Status:** OPEN -- technical debt, deliberate (design-decisions 1555,
decision 2).

**In short:** the kernel remembers where paths lead, so it does not walk
every directory of a path each time it is used. While any program has a
private set of mounts (a mount namespace), that memory is switched off for
every program, so path lookups are slower system-wide until the last such
namespace goes. Nothing is wrong, only slower.

**Where.** `fs::vfs::resolution_cacheable` and the resolution cache
(`VFS_DCACHE`, `resolve_follow`, `resolve_no_follow`); emptied by
`Vfs::copy_mount_table` and `Vfs::drop_mount_table`.

**Why.** The cache maps a path to where it resolves. With two mount tables a
path names a different file in each, and a change made through one path
(a rename, an unlink of a link) invalidates the entries under that path only;
an entry another namespace made for the same file under another path stays,
stale. Keying the cache by namespace too would fix the first and not the
second.

**The proper fix.** Key entries by what they resolve -- the filesystem and
inode of each step (`fs_id`, `ino`), as Linux's dentry cache hangs off the
inode -- and invalidate by inode, so a change through any path reaches every
entry for that file. Then the cache can serve every namespace and this
switch goes.

**Measure first.** `bench.rs`'s `vfs_resolve_*` figures with a mount
namespace alive show what the switch costs; worth doing when containers use
mount namespaces routinely.

**A kernel user, 2026-10-31.** A WiFi driver joining the network the boot
medium names runs in a sealed mount namespace of its own, which holds the
network file while it is placed (design-decisions 1596,
`linuxdrv::start_in`): the cache stands aside from the driver's start to its
end. In the boot's join that is the length of the join run, a few minutes;
a WiFi driver serving for the system's life would keep it aside for the
uptime, which makes this the trigger for the fix above.
