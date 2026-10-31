## 1595. A capability to read beneath one directory

**Date:** 2026-10-30 · **Decided by:** Claude (autonomous) -- both fixes known-issues A-LINUX-DRIVER-PROCESS-READS-FILES-UNCONFINED names as worth having; points 6 and 7 after the first review · **Lane:** A

**In short:** A Linux driver process that loads firmware -- a WiFi card's,
say -- needs to read a few files, and could only be given the right to read
every file on the system, as root. A driver is code nobody here reviewed,
and the card it drives can be turned against it; with that right, either
could read anything on disk. There is now a narrower right: to read beneath
one directory, and nothing else. Every firmware-loading driver is given that
right for the firmware directories, instead of the right to every file --
directories nobody can change, so the right stays that narrow -- and runs
as a user of its own rather than as root.

### The decision

1. **A new capability type, `ResourceType::FileTree` = 37**, with `READ`.
   Its `resource_id` names a directory in a table the kernel keeps
   (`fs::trees`): ids from 1, given by `register`, kept for a directory
   registered again, never given to another; id 0 names none. A capability
   names a tree by that id, never by a path or a hash of one -- a hash a
   process could make collide with the one it was given.
2. **Every open by name honours it, to read only** (`open_authority` in
   `syscall::handlers`: `SYS_FS_OPEN`, `SYS_FS_OPEN_MODE`, the Linux
   `open`/`openat`; not `openat2`, whose own base and confinement mean
   nothing combined with a tree's). A process holding no `File` capability
   (which is every file, as before) for the flags, but a `FileTree` one,
   opens an absolute path beneath that tree's directory -- component by
   component, `.` passed over, so `/ab` is not beneath `/a` -- with flags
   asking for no more than reading: not for writing, which truncating,
   appending and creating need too. Of several trees held, the nearest
   directory is tried first, then the others.
   The walk is the existing resolve-beneath one (`fs::handle::open_beneath`,
   `openat2`'s `RESOLVE_BENEATH`): a `..` or a symbolic link leading out of
   the directory is refused, as `PermissionDenied`, like any file outside it.
   The handle it gives is an ordinary file handle: `fstat`, `pread` and
   `close` work on it as on any other.
3. **It satisfies no other check.** Every other file syscall asks for the
   `File` type, which a `FileTree` capability is not: a holder can stat,
   list, write or delete nothing. Kept apart as its own type for exactly
   that: a `File` capability that did not count as one would be a trap for
   every reader of `has_capability_type`.
4. **Firmware-loading driver processes are given trees, not `File`.**
   `linuxdrv::firmware_reading`: the boot medium's firmware
   (`/.boot-firmware`) and the system's (`/lib/firmware`), the two
   directories Linux's loader is pointed at -- for `mt7925e` on its card,
   for a driver `linuxdrv.serve` starts that reads firmware, and for the
   hello process's boot-firmware run, which loads its firmware holding only
   them.
5. **Not requestable, inherited across fork as `File` is, no Linux
   capability** -- `CAP_DAC_READ_SEARCH` is every file; lane D is asked to
   withhold the type in init (requests/a-d-resource-type-37-is-filetree.md).
   Id 0, which elsewhere names a type as a whole, names no tree: the whole
   of files is the `File` capability (`cap` module docs).
6. **A tree is a directory that cannot change under it** (added after the
   first review, which escaped the first version three ways). `register`
   takes only a directory on a read-only filesystem -- with no file
   permissions enforced, anyone who may write a filesystem could hard-link
   any file into a directory on it -- and pins the tree to that directory's
   filesystem and inode, which every open checks again: a filesystem
   mounted over the path, or a symbolic link or another directory put where
   it was, is not the tree, which then reads nothing. The boot medium's
   firmware is placed on a memfs sealed read-only; an installed system's
   `/lib/firmware`, on its writable root, is no tree
   (known-issues A-AN-INSTALLED-SYSTEMS-FIRMWARE-IS-NO-TREE).
7. **Driver processes run as uid 65533, not root** (`linuxdrv::DRIVER_UID`;
   one that comes out root is refused). As root, a driver could mount over a
   tree's path between the check and the walk; as itself it can mount
   nothing. A third party that can mount could still race an open's check
   against its walk -- but that party is root, which needs no tree's help to
   read a file.

### Alternatives

- **An unprivileged identity per driver process alone**, ordinary
  permissions confining it -- taken as well (point 7), but not enough by
  itself: the VFS enforces no file permissions, so an identity confines
  nothing a capability does not.
- **A tree on any directory, pinned only** -- `/lib/firmware` on an
  installed root would be one, but a hard link planted in it would be read
  through it; the pin stops a mount or a swap, not a link.
- **Walking from the pinned directory itself rather than its path** -- would
  close the third party's race of point 7, but the VFS's walks all start
  from a path and take a filesystem's lock per step, so a pinned walk is a
  second resolver; the race is root's only.
- **Overloading `File` with an id range for trees** -- no new type, but
  `has_capability_type(File)` would have to learn to refuse part of its own
  type, everywhere it is asked.
- **A tree named by a path hash** -- no table, but a 64-bit hash of a path a
  process may choose can be made to collide.
- **Handing the driver its firmware's bytes rather than a right to read** --
  no file access at all, but Linux's loader asks for files by name, in
  several directories, as the driver's own code decides; the layer would
  have to answer for the file system.

### Checked by

`fs::trees::self_test` (every boot): ids kept and never zero, a directory
named with `.` or `..` or relatively refused, one on a writable filesystem
refused, the table's bound, `beneath` component by component, `.` passed
over, absolute only, no tree for id 0 or an id never given, and none once
its filesystem is unmounted. `proc::spawn::self_test_open_rights` (every
boot, ring 3), on a sealed memfs: a process holding only a tree opens a file
beneath it to read -- through a `.`, `O_RDONLY | O_CREAT` too, and beneath a
tree nested in it -- and is refused truncating or creating there, reading
the file beside it, one reached by `..`, one reached by a symbolic link out
of it, or a writable filesystem's; a nested tree does not read its
parent's; of two nested trees held, what the nearer refuses opens through
the other; a tree for another directory reads nothing there, and a tree
reads nothing once another filesystem is mounted over its directory, which
a `File` capability then reads. The linuxdrv self-test: every driver
process is started as `DRIVER_UID`; the hello process loads the boot
medium's firmware through Linux's loader holding only the firmware trees;
holding none, or only a tree for another sealed directory, it loads
nothing, its firmware initcall failing.
