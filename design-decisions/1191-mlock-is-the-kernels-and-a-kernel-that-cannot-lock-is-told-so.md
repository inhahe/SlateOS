## 1191. `mlock` is the kernel's, and a kernel that cannot lock is told so

**Date:** 2026-10-11 (answered 2026-10-09)
**Decided by:** Operator (Claude recommended B; the operator chose B, then asked about swap)
**Lane:** D

**In short:** a program can ask for memory to be locked -- kept in RAM,
never moved to swap -- as password managers and GnuPG do for keys. The C
library used to answer "done" without the kernel doing anything. The
operator chose that it say "could not lock" (`EAGAIN`) wherever the kernel
cannot. Since 2026-10-07 lane A's kernel can (`SYS_MEMORY_LOCK`, 1145), so
the library asks it, and passes its answer on; only a kernel without the
call gets the "could not lock" the operator chose.

**The operator's answer** (`operator-answers/2026-10-09-open-questions-answers.txt`,
D-Q8): "B, though the wording seems to suggest that swap only works on a
disk that's only used for swapping? Shouldn't we allow a swap file or at
least a swap partition? What did we agree on in the design files?"

**The options as asked** (D-Q8):

| Option | *What changes* |
|---|---|
| A. Keep saying "done" | programs go on believing memory is locked |
| **B. Say "could not lock" (`EAGAIN`) until the kernel can** (chosen) | nothing is told something false |
| C. Say so only when a disk swap is attached | |

**What follows.** `mlock`, `mlock2`, `munlock`, `mlockall` and `munlockall`
(`posix/src/mman.rs`) call `SYS_MEMORY_LOCK`, which takes Linux's arguments
and answers Linux's errnos in Linux's order -- the library's own pre-checks
go. On a kernel without the call (`NoSuchSyscall`) the lock calls answer
`EAGAIN` -- POSIX's "some or all of the memory could not be locked", for
`mlockall` as for `mlock` -- once their arguments pass, and the unlock calls
0, since nothing is locked.

**The swap question back.** The design agrees with the operator: a swap
*file* is the default, and a swap *partition* is a choice at manual
installation (`design.txt`: "Use a swap file as default"; the installer's
"size of swap partition or include swap file in the main partition"). The
kernel had neither -- only a spare raw disk -- and lane A has taken swap
files, swap partitions and `swapon`/`swapoff` (lane A's
`known-issues/A-SWAP-HAS-NO-SWAP-FILE-AND-CLAIMS-ANY-DISK-WITHOUT-AN-EXT4-SUPERBLOCK.md`).
D-Q8's wording described the kernel as it was, not what the design wants.
