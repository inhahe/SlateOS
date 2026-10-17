### A-A-COPY-TO-USER-MEMORY-UNDER-A-LOCK-ANSWERS-EFAULT -- 2026-10-17 -- OPEN (lane A)

**Status:** OPEN (lane A) -- found by the batch-8 reviews (Fable, Opus), of
the user-memory fault fixups (`mm::uaccess`).

**In short:** when the kernel copies to or from a program's memory while it
holds a lock -- the process table, any spinlock -- and the page faults (it
was swapped out, or a save made a shared file mapping's page read-only
again), the fault cannot be resolved there, because resolving may wait. The
copy then answers `EFAULT`, and the system call fails, where the same copy
made without the lock would have succeeded. Before 2026-10-17 such a fault
halted the kernel, so this is no worse than it was; but it is a wrong answer
a program can see, now and then, under memory pressure.

**Where:** `idt::handle_page_fault`, its `mm::uaccess` branch: a fault there
is resolved (`resolve_user_fault`) only with interrupts on and a preemption
count of 0, and a resolution that needs the process table this task already
holds answers `Busy`, which the branch also treats as unresolvable. The first
such fault of a boot is said on the serial log: `[uaccess] a copy to or from
user memory faulted where it may not wait (rip ..., address ...)`.

**Proper fix:** no copy of user memory under a lock -- Linux's rule, which it
enforces with `might_fault()`. Find the callers that copy while holding a
lock (the log names the first; a `might_fault`-style debug assertion in
`mm::user`'s copies, asserting a preemption count of 0, would name them all
at their first copy rather than their first fault), and have each copy
outside it: read what it needs under the lock, copy after.

**What happens if it is never fixed:** a system call that copies under a
lock fails with `EFAULT` when its user page happens to be swapped out or
just saved -- rare, and visible only as an occasional failure.
