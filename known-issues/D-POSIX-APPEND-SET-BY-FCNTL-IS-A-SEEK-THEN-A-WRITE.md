## D-POSIX-APPEND-SET-BY-FCNTL-IS-A-SEEK-THEN-A-WRITE — `O_APPEND` turned on after `open` is two calls, which another process's append can come between (lane D, 2026-10-01) — **Status: OPEN (lane A adds `SYS_FS_SET_STATUS_FLAGS`, 1095, after its 2026-10-01 publish; then lane D's `F_SETFL` hands it the flags)**

**In short:** a file opened for appending (`O_APPEND`) gets each write added
at the very end, even when other programs append to it too -- that is what
log files rely on. That works when the program asks for it at `open`. A
program that turns it on later, with `fcntl(fd, F_SETFL, O_APPEND)`, gets an
imitation: the library moves to the end and then writes, in two steps, so a
write by another process between the two is overwritten. One program alone
never sees a difference.

**Where:** `posix/src/file.rs`, `write`: for a file descriptor whose status
flags hold `O_APPEND`, `SYS_FS_SEEK(handle, 0, SEEK_END)`, then
`SYS_FS_WRITE`. For an `O_APPEND` the kernel was told at `open`, the seek is
redundant and harmless -- since lane A's 2026-10-01 handles, the kernel finds
the end and writes in one step. `fcntl`'s `F_SETFL` changes only this
library's copy of the flags (`fdtable::set_status_flags`), which the kernel
never sees.

**To see it:** two processes, each `open` without `O_APPEND`, then
`fcntl(F_SETFL, O_APPEND)`, then many short writes at once: some lines are
overwritten. With `O_APPEND` at `open`, none are.

**Who meets it:** anything that turns `O_APPEND` on with `F_SETFL` -- in
this tree, `fdopen(fd, "a")` on a descriptor opened without it and
`freopen(NULL, "a", f)` (`posix/src/stdio.rs`, both as musl's do), and
`dd oflag=append` with no `of=` (`userspace/coreutils/src/bin/dd.rs`).

**The proper fix, agreed with lane A on 2026-10-01:**
`SYS_FS_SET_STATUS_FLAGS(handle, flags)`, 1095, flags in the native
`OpenFlags` bits. The kernel takes `APPEND` from them and ignores the access
mode and creation bits, as Linux's `F_SETFL` does, and it acts on the open
file description, so every `dup` of the handle sees it. Then `F_SETFL` hands
`O_APPEND` to it, and `write` drops its seek -- keeping it only where the
call answers `ENOSYS`, a kernel from before it.
