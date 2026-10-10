## 1180. `fallocate` does mode 0 by writing zeros and refuses every other mode with `EOPNOTSUPP`; its checks are Linux's, measured

**Date:** 2026-10-06
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `fallocate` and `posix_fallocate` let a program reserve disk
space before writing, so that a download or a database log cannot run out
of room half-way. The C library did two things wrong. It reserved past the
end of a file by growing the file with `ftruncate`, which on this kernel
builds the whole file in kernel memory (a large reservation could halt the
machine). And it answered "done" to `FALLOC_FL_KEEP_SIZE` -- reserve without
growing the file -- having reserved nothing. Now growing a file writes
zeros from its end, a piece at a time. Every mode the library cannot really
do is refused the way Linux refuses a file system that cannot do it. The
order in which bad arguments are refused is Linux's, measured on Linux
call by call.

**1. Mode 0 grows the file by writing zeros, not by `ftruncate`.**

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. Zeros from the end, 64 KiB at a time** (chosen) | reserving 1 GB writes 1 GB | every block really allocated; `ENOSPC` when the volume fills; bounded kernel memory, through the append path | slower; races an appender, as glibc's fallback does |
| B. `ftruncate` (as before) | one call | cheap | on this kernel it builds the whole file in memory; on Linux it would allocate nothing (a hole) |
| C. The kernel's `SYS_FS_FALLOCATE` | metadata only | fast, the kernel's own reservation | takes a path, not a descriptor; succeeds silently where it cannot allocate (a deep extent tree, past 128 MiB) |

Holes inside the file are allocated the way glibc's fallback allocates
them: a zero byte written back where one reads zero. That is done only
where the block count says a hole may be. A file system that counts no
blocks (memfs, behind `/tmp`) is taken to have none.

**2. Every other mode is `EOPNOTSUPP`.** `KEEP_SIZE`, `PUNCH_HOLE`,
`ZERO_RANGE`, `COLLAPSE_RANGE`, `INSERT_RANGE` and `UNSHARE_RANGE` each
need the file system to change extents. No call reaches it by
descriptor, so the library cannot do them. `EOPNOTSUPP` is what Linux
answers on a file system that cannot, and programs fall back on it.
Writing zeros would fake `ZERO_RANGE` and `PUNCH_HOLE` well enough to read
right, but a punched hole would free nothing, and `KEEP_SIZE` past the end
cannot be faked at all. A descriptor-based fallocate from lane A would let
them work (`known-issues/D-POSIX-FALLOCATE-CANNOT-RESERVE-PAST-THE-END.md`).

**3. The checks, and their order, are Linux's, measured.** A program on
WSL2's Linux, 24 modes across five kinds of descriptor, fixed what the code
had guessed. The table is in `posix/src/file.rs`'s tests
(`LINUX_FALLOCATE`). Three things changed:

- the reserved `NO_HIDE_STALE` is `EOPNOTSUPP` before anything else;
- `UNSHARE_RANGE` with anything but `KEEP_SIZE` is `EINVAL`;
- a read-only descriptor is `EBADF`, a pipe `ESPIPE`, a terminal or an
  eventfd `ENODEV`, as on Linux. Before, these depended on what
  `ftruncate` happened to say.

Free space is not asked first. memfs and several other file systems here
report none whatever they hold, so asking would refuse growth that would
succeed.

**Where:** `posix/src/file.rs` -- `fallocate_on`, `allocate`, `fallocate`,
`posix_fallocate`. `services/ctest-fallocate` checks it on SlateOS. The
kernel half is
`requests/d-a-truncating-an-ext4-file-builds-the-whole-file-in-kernel-memory-so-truncate-s-10g-halts-the-kernel.md`.
