### A-CONTAINER-TAR-HAS-NO-FIFO-ENTRY -- 2026-10-08 (lane A)

**Status:** OPEN

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
