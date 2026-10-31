## A-AN-OPEN-THAT-CREATES-ASKS-WRITE-NOT-CREATE (lane A, 2026-10-30) — OPEN

**Status:** OPEN

**In short:** the right to make files (`Rights::CREATE`) is asked by the
calls that only make one -- `sys_fs_create`, `mkdir`, `mkfifo` -- but not by
an open that makes one (`O_CREAT`), which asks the right to write instead.
So a process given the right to read and write files, and not to create
them, can still create them by opening them.

**What is wrong.** `require_open_rights` (`kernel/src/syscall/handlers.rs`,
every open path) asks `File` `WRITE` for `OpenFlags::CREATE`, as the Linux
`O_TMPFILE` open did before it ("as every create does"). `sys_fs_create` asks
`CREATE` for the same act. Before 2026-10-30 an open asked `READ` alone for
anything (known-issues A-OPEN-WITH-ONLY-READ-RIGHTS-CAN-TRUNCATE-AND-CREATE);
asking `CREATE` there then failed fifty of the boot test's ring-3 programs --
the kernel's own test grants give `READ | WRITE` (99 of them) and the
programs `fopen(..., "w")` -- so `WRITE` it is, which closes the hole a
`READ` holder had and changes nothing for the grants as they are.

**Who sees it.** No one is wronged by it today: every grant that writes
expects to create. It matters the day something is meant to edit files but
not make them -- a sandboxed editor, a service confined to its own files.

**The proper fix.** A decision first: whether `CREATE` means "make a
directory entry by any call", which an open with `O_CREAT` is. If so, the
open asks it, and every grant meant to create says so -- the kernel's test
grants in `kernel/src/proc/spawn.rs`, and `/etc/startup.conf`'s `caps:`
lines, which init (lane D) turns into grants -- in the same change, a boot
test finding any it missed.
