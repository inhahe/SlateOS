## 959. Kernel heap allocations too big for any buddy block are mapped from vmalloc, and the kernel half's top-level page-table entries are fixed at boot

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A &middot; **Resolves:** A-Q19 (removed from `open-questions.md`; the operator may overrule)

**In short:** the kernel could not start any program bigger than 16 megabytes,
because it reads the whole program file into one block of memory and the
largest block its page allocator can hand out is 16 MB. Now, when the kernel
needs more than that in one piece, it builds the piece out of many small blocks
arranged to *look* contiguous (virtual memory — the same trick every program
already gets). Any program that fits in free memory can start. Making that safe
meant fixing four older bugs in the code that does the arranging. The most
important: memory arranged that way after the first program started was
invisible to every other program.

**What was decided.**

1. `mm::heap` sends a large allocation to `mm::vmalloc` when its buddy order
   would exceed `frame::BUDDY_MAX_ORDER` (10, i.e. over 16 MiB) —
   `HeapInner::exceeds_buddy`. **Only** those: everything the buddy allocator
   can serve is still served by it, physically contiguous and reached through
   the direct map's large pages. The requests that move are ones that used to
   fail whatever memory was free, so no existing caller changes behaviour. The
   vmalloc path runs outside the heap lock (mapping 1,400 frames must not stall
   every other slow-path allocation), refuses alignment above 16 KiB, and is
   freed by address: a pointer in the vmalloc region goes back to `vfree`.
2. The vmalloc region grows from 128 MiB to 1 GiB (`kvspace::VMALLOC`): room
   for a large program plus the second copy a growing `Vec` briefly holds.
   Virtual space is free until mapped; the bitmap is 8 KiB, and the free-run
   search now skips whole full or empty words.
3. `page_table::init` creates every kernel top-level (PML4) entry any mapper can
   reach — the direct map up to the CPU's physical address width, plus each
   region in the new `kvspace::PAGE_TABLE_MAPPED` — and then **freezes** the
   kernel half: a mapping that would need a new kernel top-level entry is
   refused, with a serial line naming it. `alloc_pml4` copies the kernel half
   from the kernel's own PML4 (`page_table::kernel_pml4_phys`) rather than from
   whatever is loaded. A build-time check refuses a kernel region that is in
   neither `PAGE_TABLE_MAPPED` nor the KASAN shadow (which `kasan::early_init`
   already handles the same way, for the same reason).
4. vmalloc and kstack map through the kernel PML4, never the loaded one.
5. Page-table entries are read and written atomically (Acquire/Release), and a
   new intermediate table is published with a compare-and-swap; the loser of a
   race frees its page and walks into the winner's.
6. `vfree` shoots down the freed range on every CPU before any frame is freed,
   and keeps the range reserved until then.
7. The direct map's top-level entries are marked no-execute in
   `page_table::init`, not at boot step 22e2 by `protect::harden_hhdm_nx` —
   which changed only the loaded PML4, long after the ring-3 self-tests had
   copied the executable originals. `harden_hhdm_nx` stays as the check and
   now reports 0. Two regions that were mapped without being registered —
   `bench_page_fault`'s window and `protect`'s mprotect test page — became
   `kvspace::BENCH` and `kvspace::PROTECT_TEST`; the freeze would otherwise
   have refused them.

**The four bugs 3–6 fix,** all in code that had exactly one caller before
today (vmalloc's own self-test), which is why none had surfaced:

| bug | consequence once the heap depends on it |
|---|---|
| a kernel top-level entry created after the first process existed went only into the loaded PML4 | a vmalloc buffer allocated during one process's syscall is unmapped in every other address space — a kernel page fault in whoever touches it next |
| mappings made through the loaded PML4 were charged to that process's RSS | a 22 MB program buffer counted against whichever process was running, and was subtracted from whichever was running when it was freed. The OOM killer ranks by RSS |
| `vfree` flushed no TLB entries, and released the range *before* unmapping it | a reused range could be read and written through the previous frame's stale translation; and a concurrent `vmalloc` could map the released range just in time for `vfree` to unmap its pages |
| `walk_or_create` stored a new table with a plain write | two CPUs creating the same table: the second store discards the first table and the mapping already in it |

kstack had the first two as well, invisibly: every kernel stack allocated after
boot was charged to the running process, and it passed the raw CR3 value — PCID
bits included — where a table address belongs, harmless only while every CR3
carries PCID 0.

**Alternatives considered** (A-Q19's three, and one more):

| option | why not |
|---|---|
| **Raise `MAX_ORDER`** to 11 or 12 | touches the allocator every frame goes through and buys one or two doublings; the next large port meets the same wall. It would also make the largest contiguous block a standing demand on physical memory |
| **Stream the ELF from the file** instead of holding it (A-Q19's recommendation) | the better memory profile — the file is held once, in the page cache, rather than once in the kernel and again as segments — but a rewrite of a loader with 69 uses of the slice. It is still worth doing, as an optimisation; it is no longer needed for correctness, and this does not make it harder |
| **Accept the limit** | writes off every port over 16 MB, `gcc` included, for a limit that was never a design choice |
| **Also fall back to vmalloc when the buddy allocator fails for an order ≤ 10** (fragmentation), as Linux's `kvmalloc` does | tempting, since the alternative is an allocation failure. Not done because it moves *currently working* allocations — which might be freed with interrupts disabled — onto a path whose free does a cross-CPU TLB shootdown, and that deadlocks if a CPU waits for it with interrupts off. Worth revisiting together with the shootdown's interrupts-off hazard |
| **Grow the direct map's top level lazily and copy each new entry into every PML4** (Linux's `sync_global_pgds`) | needs a registry of every address space and a lock ordering with process creation. Creating the direct map's entries up front costs one 4 KiB page per 512 GiB of physical address space — 8 KiB on this QEMU (40-bit), at most about 0.5 MiB on a 48-bit machine |

**What it does not change.** Buddy-served allocations behave exactly as before,
including the power-of-two rounding that makes an 8 MiB + 1 byte request take a
16 MiB block. Syscalls whose kernel buffers are sized by the caller can now
reach the 1 GiB vmalloc region instead of failing at 16 MiB; the sites found
by the 2026-09-21 audit already allocate fallibly, and bounding them properly is
tracked in known-issues (`A-USER-SIZED-KERNEL-BUFFERS-NOW-REACH-VMALLOC`).

**Where this bites:** `kernel/src/mm/heap.rs` (`exceeds_buddy`,
`alloc_virtual`, `dealloc_virtual`, `virtual_alloc_self_test`),
`kernel/src/mm/vmalloc.rs`, `kernel/src/mm/page_table.rs` (`init`,
`kernel_pml4_phys`, `walk_or_create`, `install_table`, `walk_or_create_pml4`,
`mark_direct_map_no_execute`, `alloc_pml4`), `kernel/src/mm/kvspace.rs`
(`PAGE_TABLE_MAPPED`, `VMALLOC`, `PROTECT_TEST`, `BENCH`),
`kernel/src/mm/kstack.rs`, `kernel/src/mm/hugepage.rs`, `kernel/src/mm/cow.rs`,
`kernel/src/mm/protect.rs` (`harden_hhdm_nx`), `kernel/src/bench.rs`,
`kernel/src/cpu.rs` (`physical_address_bits`), `kernel/src/syscall/linux.rs`
(`epoll_wait_core`), `kernel/src/proc/spawn.rs` (the cmake rung, which now
runs).
