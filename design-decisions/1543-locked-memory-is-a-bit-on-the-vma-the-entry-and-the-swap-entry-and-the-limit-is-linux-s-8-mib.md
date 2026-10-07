## 1543. Locked memory is a bit on the VMA, the entry and the swap entry, and the limit is Linux's 8 MiB

**Date:** 2026-10-07 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** `mlock` and its relatives now really keep memory in RAM: the
kernel never swaps a locked page out, so a key a program locked never
reaches the compressed swap pool or a swap disk. Until now both system call
interfaces said "done" and locked nothing (lane D's
`requests/d-a-mlock-locks-nothing.md`). Three choices had another reasonable
answer, and one of them is visible to programs: **a process may now lock at
most 8 MiB unless it holds the right to lock more**, where before the limit
was "unlimited" -- which cost nothing while locking did nothing.

**The model** (`kernel/src/mm/mlock.rs`): one software bit,
`PageFlags::MLOCKED` (PTE bit 11), in three places -- on a VMA's flags, which
every page faulted into it is mapped with; on each present page table entry,
which is what the reclaimer reads; and, as `swap::SWAP_KEPT_LOCKED` (bit 44),
on a swap entry, so a page that was already swapped out when its range was
locked comes back locked by whichever path brings it back. The errnos, their
order and every edge (wrap `EINVAL`, a hole `ENOMEM` with the part before it
changed, `PROT_NONE` `ENOMEM` with the lock kept, `MCL_FUTURE` forgotten by a
later `mlockall` without it, `mmap` under it `EAGAIN` past the limit) are
Linux 6.6's, checked by one ring-3 program on both
(`spawn::self_test_linux_mlock`).

**Choice 1 -- `RLIMIT_MEMLOCK` defaults to 8 MiB, Linux's default since
5.16, not unlimited.** *What changes:* a program that locks more than 8 MiB
without the `MEMORY_LOCK` right gets `ENOMEM` where it used to get a success
that meant nothing. Unlimited would keep every such call succeeding, but as a
limit on real pinning it lets any process hold every frame of RAM out of the
reclaimer's reach -- a denial of service any program could commit. Programs
that need more (databases, audio servers) are exactly the ones Linux also
makes ask (`CAP_IPC_LOCK`, here the `MEMORY_LOCK` right `init` holds, or a
raised limit), so they already know how. *Easy to reverse:* the default is
one tuple in `pcb.rs`'s rlimit table.

**Choice 2 -- the reclaimer passes a locked frame by, rather than moving it to
a list it never scans.** Linux keeps mlocked pages on an "unevictable" LRU so
that reclaim never looks at them. Here the reclaim list is one clock list, and
a locked frame stays on it: each sweep pays a look at it (four entry reads),
and `munlock` has nothing to put back -- the frame is a candidate again the
moment its last part is unlocked. A separate list would need every unlock path
(`munlock`, `munlockall`, unmapping, exit) to move frames back, each one a
chance to strand a frame off every list for good; the cost it saves is bounded
by the limit (8 MiB is 512 frames a process) unless a privileged process
locks gigabytes. *Revisit when* a profile shows sweeps dominated by locked
frames.

**Choice 3 -- the main stack, which has no VMA here, grows locked when the
page it grows from is locked.** Linux's stack is a VMA that grows downward
keeping its flags, so `mlockall(MCL_CURRENT)` (or an `mlock` reaching its
bottom) locks its future growth, and `MCL_FUTURE` alone does not. Asking the
nearest stack page above the fault reproduces exactly that without giving the
stack a VMA, and without the page-fault path taking the process table. The
alternative -- a per-process "stack locked" flag -- would have needed every
`mlock`/`munlock` to work out whether its range reached the stack's bottom.

**Related fixes made on the way** (not decisions, recorded so they are not
mistaken for them): the reclaimer's second-chance test now clears each 4 KiB
entry's accessed bit by itself, atomically (it wrote the first entry's flags
into all four of a frame, which could make a part still shared copy-on-write
with another process writable); `mincore` answers per 4 KiB page from the page
tables, revealing a file page's cache residency only for a mapping that may
write the file, as Linux limits it (`can_do_mincore`), and reports any other
file page resident.
