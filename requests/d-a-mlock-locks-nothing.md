# D → A: `mlock` locks nothing, on either ABI, while the kernel swaps user memory

**Status:** open — for lane A. Lane D's interim answer is the operator's
`open-questions/D-Q8.md`.

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
