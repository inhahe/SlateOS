### [D] B-D-PREAD-MOVES-THE-SHARED-FILE-POSITION — 2026-09-26 — OPEN

**Where:** `posix/src/file.rs`: `pread`, `pwrite`, and `at_position` under
`preadv`, `pwritev`, `preadv2` and `pwritev2` -- and so kernel AIO's reads
and writes of files.

**What it is.** The kernel has no positional read or write, so these save
the descriptor's position, seek, transfer and seek back. For the length of
the call the shared position is wrong: another thread reading, writing or
`lseek`ing the same descriptor meanwhile uses the moved position, and a
second `pread` on it at the same moment can put the first one's position
back under it. Linux's `pread` never touches the position.

**The proper fix.** Positional transfers in the kernel's file interface --
its VFS already reads and writes at an offset (`read_at`/`write_at`, which
`copy_file_range` uses, and the Linux-ABI `pread64` reaches) -- exposed as
native syscalls, and `pread` built on them: lane A's, asked for in
`requests/d-a-positional-file-read-and-write.md`.
