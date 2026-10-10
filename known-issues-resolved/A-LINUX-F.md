### A-LINUX-F_SETFL-O_APPEND-REACHES-NOTHING -- 2026-10-02 -- FIXED (lane A)

**Status:** FIXED on lane-a-wip 2026-10-02, awaiting a boot -- found while
giving append-only files Linux's `setfl` rule, and fixed with it: the ring-3
`Linux file flags` test sets `O_APPEND` by `F_SETFL`, rewinds, writes, and
finds the byte at the end.

**In short:** `fcntl(fd, F_SETFL, O_APPEND)` on a Linux descriptor answers 0
and `F_GETFL` then shows `O_APPEND`, but the writes do not start appending:
the flag is recorded in the descriptor's entry and never reaches the open
file (`fs::handle`'s `OpenFlags::APPEND`), which is what every write
consults. Clearing it likewise changes nothing. The native
`SYS_FS_SET_STATUS_FLAGS` reaches the open file and is right.

**Where:** `kernel/src/syscall/linux.rs` `sys_fcntl`, `F_SETFL` arm.

**Proper fix:** for a file descriptor, set the open file's `APPEND` through
`fs::handle::set_status_flags` (which also refuses a change on an
append-only file, `EPERM`) before recording the flag in the entry, so a
refusal leaves both as they were; a directory keeps answering 0, as on Linux.
