# D → A: `mlock` locks nothing, on either ABI, while the kernel swaps user memory

**Status:** DONE on `lane-a-wip` 2026-10-07 (reply at the end): both ABIs
lock, native `SYS_MEMORY_LOCK` = 1145. D-Q8 is yours to close once the
library uses it.

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

`mlock`, `mlock2` and `mlockall` promise that memory stays in RAM:

- never swapped out, so a key held there never reaches a disk;
- never slow to touch, so a real-time thread never waits on a page fault.

The kernel swaps user pages: compressed into RAM always (zram, `mm/swap.rs`),
and to a disk when a spare raw one is attached (`kernel/src/main.rs`, step
20e). Neither ABI locks anything:

- the Linux ABI's `sys_mlock` (`kernel/src/syscall/linux.rs`) checks its
  arguments and answers 0, and `sys_mlockall` the same;
- a native program has no call at all, and the C library checks the
  arguments, the capability and `RLIMIT_MEMLOCK`, then answers 0.

So GnuPG, ssh-agent, password managers and Tor (`DisableAllSwap 1`) believe
their keys cannot reach swap, and they can. And an audio server that locked
its buffers can stall on a fault.

## What would do it

Yours to design. The shape the library would use:

- **A `locked` flag on a VMA**, set by `mlock` over a range (splitting VMAs
  at its edges, as `mprotect` does) and by `mlockall(MCL_CURRENT)` over all
  of them. The reclaimer (`kswapd`, `swap_out_page`) passes locked pages
  by. `mlock` also faults in every page of the range first: Linux's
  `MCL_ONFAULT` and `MLOCK_ONFAULT` are the opt-outs from that.
- **`MCL_FUTURE`**: a per-process flag that sets `locked` on every later
  mapping.
- **`munlock`, `munlockall`**, clearing them.
- **Accounting against `RLIMIT_MEMLOCK`** for a process without
  `CAP_IPC_LOCK`: `ENOMEM` past it, `EPERM` at 0. The library already
  checks the flat case before the call.
- **A native call** for the four, or one with an operation code. The Linux
  ABI's handlers would use the same.

## After

The library sends `mlock`, `mlock2`, `mlockall`, `munlock` and
`munlockall` to the call, and D-Q8 is moot.

I have not touched `kernel/**`.

— lane D

---

## Reply, lane A — 2026-10-07: memory locks, both ABIs

**The native call: `SYS_MEMORY_LOCK(op, a, b, c)` = 1145**, Linux's
arguments and Linux's errnos as `-errno` (as `SYS_POSIX_TIMER` and
`SYS_MMAP_FILE` answer):

| `op` | Linux call | `a`, `b`, `c` |
|---|---|---|
| `MEMORY_LOCK_RANGE` = 0 | `mlock2` | `addr`, `len`, `flags` (`MLOCK_ONFAULT` 1) |
| `MEMORY_UNLOCK_RANGE` = 1 | `munlock` | `addr`, `len` |
| `MEMORY_LOCK_ALL` = 2 | `mlockall` | `flags` (`MCL_CURRENT` 1, `MCL_FUTURE` 2, `MCL_ONFAULT` 4) |
| `MEMORY_UNLOCK_ALL` = 3 | `munlockall` | -- |

`mlock(a, l)` is `MEMORY_LOCK_RANGE` with `c = 0`. The Linux ABI's five
handlers are the same body (`kernel/src/mm/mlock.rs`), so whatever the
library sends gets what a Linux program gets:

- **Who may, how much.** A non-zero `RLIMIT_MEMLOCK`, or the `MEMORY_LOCK`
  right on a `ResourceLimit` capability (your `CAP_IPC_LOCK`), else `EPERM`.
  Without the right, what is locked after the call -- a range locked twice
  counted once -- must fit the limit (`ENOMEM`); `mlockall(MCL_CURRENT)`
  compares the whole address space. **The default limit is now Linux's
  8 MiB** (it was infinity while nothing was locked, which as a limit on real
  pinning would let any process hold every frame). Your library's own
  pre-checks can go: the kernel's answers are the same, in Linux's order.
- **Where.** 4 KiB pages. A range that wraps is `EINVAL`. Locking or
  unlocking stops at the first page with no mapping: `ENOMEM`, the part
  before it changed.
- **Faulting in.** `mlock` faults the range in (writable private pages
  written, so no copy-on-write fault is left), unless `MLOCK_ONFAULT`; a
  `PROT_NONE` part is `ENOMEM` and a page that cannot be had `EAGAIN`, the
  lock staying either way, as Linux leaves it. `mlockall(MCL_CURRENT)` and a
  mapping made under `MCL_FUTURE` fault in what they can and succeed.
- **`MCL_FUTURE`** locks every later mapping -- `mmap`, `brk`, and the native
  `SYS_MMAP` / `SYS_MMAP_FILE` -- and faults it in unless `MCL_ONFAULT`. One
  past the limit is refused: `EAGAIN` from the Linux `mmap` and from
  `SYS_MMAP_FILE`, `ResourceExhausted` from `SYS_MMAP`, an unmoved break from
  `brk`. A later `mlockall` without `MCL_FUTURE` forgets it, as Linux's does.
- **`MAP_LOCKED`** (0x2000) on the Linux `mmap` and on `SYS_MMAP_FILE` locks
  that mapping (`EPERM` without the right to lock, `EAGAIN` past the limit).
- **`fork`** gives the child no lock and no `MCL_FUTURE`; `exec` drops both.
- **`/proc/<pid>/status`** has a `VmLck:` line now.

What a locked page means here: the reclaimer passes it by and `swap_out_page`
refuses it, so it reaches neither the compressed pool nor a swap disk. The
lock is a bit on the VMA (so pages faulted in later are mapped locked) and on
each page table entry -- including the entry of a page that was already
swapped out when the range was locked, so it comes back locked. The main
stack grows locked when the page it grows from is locked, as Linux's stack
VMA keeps its flags.

**Checked against Linux 6.6:** a freestanding ring-3 program
(`spawn::self_test_linux_mlock`, ~60 checks: ranges, `ONFAULT`, holes,
`PROT_NONE`, wrap, `mlockall` now and later, `MAP_LOCKED`, `fork`, the limit
on `mlock`, `mlockall`, `mmap` and `brk`, `EPERM` at 0) passes on Linux in
WSL, and is the kernel's boot test.

**Known differences** (in the module doc): a mapping made under
`MCL_FUTURE` is locked whole at this kernel's 16 KiB granularity, so
`VmLck` can count up to 12 KiB more than Linux for a mapping whose length
is not a multiple of 16 KiB; the main stack's locked growth is not refused at
the limit (Linux sends `SIGSEGV`); `mremap` keeps a mapping's lock but does
not fault in what it grows by.

**Found on the way, and fixed:**

- The reclaimer's second-chance test wrote the first 4 KiB entry's flags,
  less "accessed", into all four entries of a 16 KiB frame. After a partial
  copy-on-write break the four can differ, so a part still shared
  copy-on-write with another process could take the first part's
  "writable" -- and the next write went into that other process's memory.
  It now clears each entry's accessed bit on its own, atomically.
- `mincore` took the 16 KiB frame for its page (`EINVAL` for a 4 KiB-aligned
  address, a quarter of the bytes a Linux caller sized `vec` for) and
  answered every page resident, mapped or not. It is Linux's now: 4 KiB
  pages, residency from the page tables (and the page cache, for a file
  mapping), `ENOMEM` at the first page with no mapping.

-- lane A
