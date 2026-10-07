## D-POSIX-FALLOCATE-CANNOT-RESERVE-PAST-THE-END — `fallocate` answers `EOPNOTSUPP` for every mode but 0, and `posix_fallocate` allocates by writing zeros (lane D, 2026-10-06)

**Status:** OPEN -- waiting on lane A
(`requests/d-a-truncating-an-ext4-file-builds-the-whole-file-in-kernel-memory-so-truncate-s-10g-halts-the-kernel.md`,
"Lane D's side").

**In short:** `fallocate` lets a program reserve disk space ahead of
writing -- a download or a database's log can make sure the space is there
before it starts -- and, with other modes, punch holes or zero ranges. The
kernel has no call that does any of that by file descriptor, so the C
library does what it can with ordinary writes:

- **Mode 0** (`posix_fallocate`, or `fallocate` with mode 0) grows the file
  by writing zeros from its end, and allocates holes inside the range by
  writing back a zero byte where one reads zero. The space is really
  allocated. But it costs a write of every byte, races another process
  writing the same range (as glibc's fallback does), and on this kernel a
  write into a hole is `EIO`, which is then the answer.
- **Every other mode** -- `FALLOC_FL_KEEP_SIZE` (reserve past the end
  without growing the file), punching holes, zeroing, collapsing and
  inserting ranges -- is `EOPNOTSUPP`, which is Linux's answer where a file
  system cannot do it. Programs fall back. Until 2026-10-06 `KEEP_SIZE`
  answered 0 having reserved nothing.

**Where:** `posix/src/file.rs` -- `fallocate_on`, `allocate`,
`posix_fallocate`, `fallocate`.

**The proper fix:** a kernel call that allocates by descriptor with
Linux's modes (at least mode 0 and `KEEP_SIZE`; `PUNCH_HOLE`,
`ZERO_RANGE` after), allocating extents without writing data, refusing
with `ENOSPC` when it cannot -- not the path-based `SYS_FS_FALLOCATE`,
which is silent where it cannot allocate. Then `allocate` calls it, and
the modes it supports stop being `EOPNOTSUPP`.

**Reproduce:** `fallocate(fd, FALLOC_FL_KEEP_SIZE, 0, 4096)` on a file open
for writing: -1, `EOPNOTSUPP`.
