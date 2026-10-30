## 1595. A capability to read beneath one directory

**Date:** 2026-10-30 · **Decided by:** Claude (autonomous) -- the second of the two fixes known-issues A-LINUX-DRIVER-PROCESS-READS-FILES-UNCONFINED names as worth having; the first, an unprivileged identity per driver, waits on lane B's user database · **Lane:** A

**In short:** A Linux driver process that loads firmware -- a WiFi card's,
say -- needs to read a few files, and could only be given the right to read
every file on the system, as root. A driver is code nobody here reviewed,
and the card it drives can be turned against it; with that right, either
could read anything on disk. There is now a narrower right: to read beneath
one directory, and nothing else. Every firmware-loading driver is given that
right for the firmware directories, instead of the right to every file.

### The decision

1. **A new capability type, `ResourceType::FileTree` = 37**, with `READ`.
   Its `resource_id` names a directory in a table the kernel keeps
   (`fs::trees`): ids from 1, given by `register`, kept for a directory
   registered again, never given to another; id 0 names none. A capability
   names a tree by that id, never by a path or a hash of one -- a hash a
   process could make collide with the one it was given.
2. **`SYS_FS_OPEN` honours it, to read only.** A process holding no `File`
   capability (which is every file, as before) but a `FileTree` one opens
   an absolute path beneath that tree's directory -- component by
   component, so `/ab` is not beneath `/a` -- with flags asking for no more
   than reading (no write, create, truncate, append or exclusive create).
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

### Alternatives

- **An unprivileged identity per driver process**, ordinary permissions
  confining it -- worth having too, and the known issue says so; it needs a
  driver user agreed with lane B, and it confines by what the file system's
  owners allowed, where a tree confines by what the process was given.
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
named with `.` or `..` or relatively refused, and `beneath` component by
component, absolute only, no tree for id 0 or an id never given.
`proc::spawn::self_test_open_rights` (every boot, ring 3): a process holding
only a tree opens a file beneath it to read, and is refused truncating or
creating there, reading the file beside it, one reached by `..`, or one
reached by a symbolic link out of it; a tree for another directory reads
nothing there. The linuxdrv self-test's boot-firmware runs: the hello
process loads the boot
medium's firmware through Linux's loader holding only the firmware trees;
holding none, or only a tree for another directory, it loads nothing, its
firmware initcall failing.
