## D-POSIX-APPEND-SET-BY-FCNTL-IS-A-SEEK-THEN-A-WRITE — `O_APPEND` turned on after `open` is two calls, which another process's append can come between (lane D, 2026-10-01) — **Status: OPEN -- both halves are written: lane A's `SYS_FS_SET_STATUS_FLAGS` (1095, on `lane-a`) and lane D's `F_SETFL` on it (2026-10-07). It closes when lane A's kernel is on `main` and `services/ctest-fsetfl-append` passes in a lane D boot.**

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

**Lane D's half, done 2026-10-07:** `F_SETFL` on a file gives the kernel its
open description's append flag (`posix/src/fcntl_ops.rs`,
`give_kernel_append`), and once the call has worked in a process, `write`
leaves `O_APPEND` to the kernel and no longer seeks first (`file.rs`,
`fcntl_ops::kernel_keeps_append`); on a kernel without the call the seek
emulates it, as before. The status flags are the description's now in the
library's table too: `F_SETFL` on one descriptor changes every descriptor
sharing its handle, so `F_GETFL` on a `dup` agrees, as Linux's does
(`fdtable::set_status_flags`). `services/ctest-fsetfl-append` checks the
two halves together -- two processes appending through `F_SETFL` lose no
line -- and passes on Linux.

**The proper fix, agreed with lane A on 2026-10-01:**
`SYS_FS_SET_STATUS_FLAGS(handle, flags)`, 1095, flags in the native
`OpenFlags` bits. The kernel takes `APPEND` from them and ignores the access
mode and creation bits, as Linux's `F_SETFL` does, and it acts on the open
file description, so every `dup` of the handle sees it. Then `F_SETFL` hands
`O_APPEND` to it, and `write` drops its seek -- keeping it only where the
call answers `ENOSYS`, a kernel from before it.
