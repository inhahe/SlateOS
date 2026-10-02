### A-LSTAT-REPORTED-NOTHING-ON-FAT-PROCFS-DEVFS -- 2026-10-02 -- FIXED (lane A)

**Status:** FIXED on lane-a-wip 2026-10-02, awaiting a boot.

**In short:** `ls -l` on a FAT volume dated every file 1970 and showed no
owner or mode: `ls` lists by `lstat`, and `lstat` reached the `FileSystem`
trait's default `lmetadata`, which built an empty record for every name. Any
filesystem without its own `lmetadata` -- FAT, which has no symlinks to need
one, procfs, devfs, the overlay -- answered that way, while `stat` of the
same file answered fully. It also hid FAT's read-only bit from the
immutable rules, which look at a name without following it.

**Fix:** the default gives `metadata` for anything but a symlink
(`vfs.rs`, `FileSystem::lmetadata`). A symlink on such a filesystem still
gets the minimal record; none of the four has symlinks of its own except
procfs (`/proc/self`), whose record was minimal before too.
