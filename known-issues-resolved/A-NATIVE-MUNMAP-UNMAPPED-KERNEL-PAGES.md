### [A] A-NATIVE-MUNMAP-UNMAPPED-KERNEL-PAGES: any process could unmap kernel memory with native `SYS_MUNMAP`, and free its frames -- 2026-09-26
**Status:** FIXED on lane-a 2026-09-26, awaiting a boot. Found by lane D reading the code.

**In short:** the native `munmap` system call took the address and length a
program gave it and unmapped every page in that range -- without checking that
the range belonged to the program. The top half of every address space is the
kernel's own memory, shared by all processes, so any program, holding no
permission at all, could unmap pieces of the kernel (kernel stacks, device
registers, anything not mapped with large pages) and hand that memory back to
the allocator while the kernel was still using it. `SYS_SHM_UNMAP` is the same
function and had the same hole. The Linux-compatible `munmap` was not affected.

**What else was wrong in the same function** (all fixed with it):

| defect | consequence |
|---|---|
| frames freed *before* the TLB shootdown | on SMP, another CPU running a thread of the process could keep writing through its stale TLB entry into a frame already handed to someone else |
| loop over a caller-chosen length with unchecked `vaddr + i * FRAME_SIZE` | a debug kernel panics on the overflow; a release kernel wraps into low memory |
| only a VMA starting exactly at `vaddr` was removed | a partial unmap left the VMA covering pages the process gave back, so the next touch demand-paged fresh memory in instead of faulting |
| `len == 0` returned 0 | POSIX and Linux say EINVAL |

**The fix.** `syscall::handlers::munmap_range` refuses, before anything is
touched: a start not on a 16 KiB frame (`BadAlignment`), `len == 0`, a length
whose rounding or whose end overflows, and any byte at or above
`USER_SPACE_END` (`InvalidArgument`). The teardown is
`mm::user::unmap_user_range`, now shared by every syscall that unmaps user
memory (native `munmap`/`shm_unmap`; the Linux ABI's `munmap`, `brk` shrink,
`MADV_DONTNEED`, and anonymous and file `MAP_FIXED` replacement): it clamps
its range to the user half itself
-- so the next caller that forgets to validate still cannot reach the kernel
half -- clears PTEs at 4 KiB granularity, batches one TLB shootdown through
`mm::tlb_gather::TlbGather`, and frees each frame (refcount-aware) only after
the shootdown and only when no sub-page of it is still mapped. The Linux path
used to send one shootdown IPI per 4 KiB page; it now sends one per range. VMAs
are trimmed with `pcb::remove_vma_range`.

**Tests.** `proc::spawn::self_test_munmap_abi` runs a ring-3 probe program
(`proc::elf::build_munmap_abi_test_elf`) holding no capability: kernel half,
a range past `USER_SPACE_END`, `addr + len` overflow, length-rounding overflow,
zero length and misalignment refused; an empty user range and the last user
frame accepted. Its kernel-half address is deliberately *unmapped* (the hole
between the KASAN shadow and kernel text), so a regression is caught without
unmapping anything. `mm::user::self_test_unmap_user_range` puts a real mapped
kernel page (a `vmalloc` allocation) in front of the teardown -- directly and
inside a range starting in the user half -- and checks it survives; and checks
the last-sub-page rule through RSS accounting.

**ABI note for callers.** Native `munmap(addr, 0)` is now `InvalidArgument`
where it returned 0. Lane D (posix libc) asked for exactly that; lanes B/C/E
native callers that relied on a zero-length unmap succeeding get EINVAL.
