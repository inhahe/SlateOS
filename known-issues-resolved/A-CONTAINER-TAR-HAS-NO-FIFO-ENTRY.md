### A-CONTAINER-TAR-HAS-NO-FIFO-ENTRY -- 2026-10-08 (lane A)

**Status:** FIXED 2026-10-09 -- fixed on lane-a-wip 2026-10-08, on main since b083cfeca (lane A's publish of 2026-10-09, boot-tested green at 3d83e0264)
(named pipes themselves are not on main yet either).

**In short:** `container export` (Docker's `export`) writes a container's
files into a tar archive. A named pipe in the container is left out of the
archive, where GNU tar keeps one (as an entry of type `6`), so a container
re-imported from the archive is missing it.

**Where:** `kernel/src/fs/tar.rs` has no entry kind for a FIFO -- its
`EntryKind` reads and writes files, directories and symlinks -- and
`container::tar_tree` skips `EntryType::Fifo` rather than archive it as the
regular file it is not. Extraction (image layers, `container import`) has the
same gap: a type-`6` entry is not made as a FIFO.

**Proper fix:** an `EntryKind::Fifo` in `fs::tar` (typeflag `b'6'`, no data,
the mode kept); `tar_tree` writes one for `EntryType::Fifo`; extraction makes
one with `Vfs::mknod_fifo`. Character and block devices (`'3'`, `'4'`) are the
same shape and would come with it.

**The fix (lane-a-wip).** That, for FIFOs: `EntryKind::Fifo`;
`container::tar_tree` archives one; `container::untar_tree` (`container
import`, `cp` of a directory), `oci::extract_layer` (image layers) and the
kernel shell's `tar -x` make one with its mode. Tests: `fs::tar`'s round trip
(typeflag `'6'`), and the container export and import self-tests. Devices are
still skipped: the VFS cannot make a device node from an archive. That is a
separate gap, and no image this kernel has pulled carries one.
