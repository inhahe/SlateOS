### BUG-MUNMAP-NO-TLB-FLUSH. `sys_munmap`/`sys_mmap` never flushed the TLB → freed frame stayed writable via stale TLB → buddy free-list corruption → kernel #PF — 2026-07-22 — ✅ RESOLVED 2026-07-22

**What:** `sys_munmap` (kernel/src/syscall/handlers.rs) walked the range calling
`page_table::unmap_frame`, which only *clears the page-table entries* and
(per its own doc-comment) leaves TLB invalidation to the caller — but
`sys_munmap` never flushed the TLB. It then immediately returned each frame to
the buddy allocator via `frame::free_frame`. The committed-`sys_mmap` path had
the mirror-image gap: `map_committed_range` also documents "caller must flush",
and the handler didn't.

**Symptom / repro:** Under heavy anonymous mmap churn (the fastpy runtime's
malloc grows/shrinks the heap by repeatedly `mmap`/`munmap`-ing single frames —
visible in the serial log as alternating `[mmap] Committed mapped …` /
`[mmap] Unmapped 1 frames at …` on the *same* VA), the process kept a stale
VA→frame TLB entry after `munmap`. Because the frame was already freed, its
first 16 bytes were reused as the buddy allocator's intrusive `FreeNode`
(next/prev physical-address links). The process's still-cached writes to the
old VA (heap pointers in the `0x60_0000_0000` mmap region) overwrote that node,
so a *user virtual address* leaked into the free list as a bogus "physical"
next-pointer. A later `alloc_frame` walked the list and dereferenced
`phys_to_virt(0x6000070000)` = `0xffff806000070008` (physmap alias of a
~412 GB "frame" that doesn't exist) → **unrecoverable kernel #PF, write,
not-present** in `BuddyAllocator::remove_free` ← `pop_free` ← `alloc_inner`.
Triggered on-target by the fastpy package-manager generations self-test
(`self_test_fastpy_slateos_pkg_gen`), whose commit/rollback file copies do
enough heap churn to cross the threshold; the lighter dependency-lifecycle test
that runs just before it did not.

**Fix:** Added `mmap_flush_range(start, end)` in handlers.rs (mirrors
`mprotect_flush_range`: per-page `crate::tlb::flush_range` for ≤64×4 KiB pages,
`crate::tlb::flush_all` above that) and call it (a) in `sys_munmap` after the
unmap loop, before the frames can be recycled, and (b) in the committed
`sys_mmap` path after `map_committed_range`. This closes the window where a
freed frame remains reachable through a stale translation.

**Why it lay hidden:** most earlier ring-3 tests either did little heap churn or
happened not to reuse a frame while its stale TLB entry was still live; the
generations test was the first to reliably cross the reuse-while-stale window.

**Class audit (2026-07-22):** swept every other unmap-then-free caller for the
same missing-flush gap. All are already correct:
- `linux.rs::sys_munmap` (Linux ABI, ~7104) and `sys_brk` shrink (~7321) go
  through `unmap_user_range`, which flushes per-page (`tlb::flush_range(va,1)`,
  ~10878) as it clears each PTE. `sys_mremap` never unmaps (returns ENOMEM).
- Rollback paths — `drm_mmap_dumb_rollback` (~10665), `linux_file_mmap_rollback`
  (~10922), the MMIO-map rollback (handlers.rs ~894/914), and the shm_map
  rollback (handlers.rs ~2801) — undo state created earlier in the *same*
  syscall. Userspace never runs between the map and the unmap, and file data is
  read into frames via the HHDM alias (not the user VA), so no TLB entry is ever
  cached; device/MMIO and shm frames are ref-decremented, not hard-freed, so
  even a stale entry couldn't corrupt the buddy free list. `linux_file_mmap_fill`
  frees (11314/11335/11343) are for frames that were never mapped.
- Process teardown (pcb.rs `clear_user_address_space`) is covered by the CR3
  reload on the next context switch, which flushes the whole TLB.
The native `handlers.rs::sys_munmap`/`sys_mmap` (fixed above) was the only path
where userspace had run and dirtied TLB entries before freeing to the allocator.
