## 19. User mmap allocator — split the window into a VMA-tracked general region and a disjoint device region

**Date:** 2026-06-14
**Decided by:** Claude (autonomous)

**Problem:**
The old `mmap_alloc_vaddr` was a single process-global `static NEXT_VADDR:
AtomicU64` monotonic bump counter handing out user mmap addresses. Three
defects: (1) it never reused a `munmap`'d address → a map/unmap-heavy process
eventually exhausts the window (permanent OOM); (2) one counter was shared
across *all* processes; (3) it never consulted the per-process VMA list, so a
returned address could overlap a `MAP_FIXED` overlay, the ld.so base, or a PIE
segment. This also blocked TD9 (ASLR), which needs a real region allocator.

**Decision:**
Replace the bump counter with a **per-process VMA-aware gap allocator**, and
split the user mmap window `0x0060_…0000 .. 0x0070_…0000` into two **disjoint**
sub-regions:

- **General region** (`USER_MMAP_BASE..USER_MMAP_END`, the low 15/16ths):
  served by `mm::vma::find_gap` (bottom-up first-fit over the sorted VMA list)
  via `pcb::reserve_unmapped_area`, fronted by
  `handlers::alloc_user_mmap_reserve`. Every mapping placed here registers a
  VMA, so freed gaps are *reused* and a returned address can never overlap an
  existing mapping. Used by anonymous mmap (committed + lazy) and file-backed
  mmap.
- **Device region** (`DEVICE_MMAP_BASE..DEVICE_MMAP_END`, the top 1/16th):
  served by the old bump allocator, now repurposed/bounded to this window.
  Used by DRM dumb-buffer mmap and MMIO mmap — mappings that map device frames
  **without** registering a VMA, so the gap finder cannot see them.

Find+insert is done atomically under one `PROCESS_TABLE` lock
(`reserve_unmapped_area` = `find_gap` + `Vma` insert), closing the SMP
find-then-add TOCTOU race (two concurrent same-process mmaps could otherwise
pick the same gap → spurious `ENOMEM` on the second `add_vma`).

**Alternatives considered:**
- *Migrate everything to the gap finder (one region).* Rejected: DRM/MMIO maps
  register no VMA, so they're invisible to `find_gap` and would collide with
  gap-finder allocations. Making them register VMAs would perturb the DRM
  frame/refcount/fork lifecycle (TD11) — a much larger, riskier change. The
  disjoint device window sidesteps this entirely.
- *Keep the bump allocator but make it per-process.* Fixes the cross-process
  bug but still leaks freed VAs and still ignores the VMA list (overlap risk).
  Rejected.
- *Separate find-gap call then `add_vma` (two lock acquisitions).* Simpler but
  reopens the SMP TOCTOU race the old atomic counter didn't have. Rejected in
  favour of the single-lock `reserve_unmapped_area` (a find-only
  `find_unmapped_area` helper was written and then removed to keep the racy
  pattern from being reintroduced).

**Tradeoff accepted:** the device region still uses a no-reuse bump allocator,
so a DRM/MMIO map/unmap-heavy process could exhaust the top 1/16th of the
window. Accepted because device buffers are few and long-lived; reuse there is
tracked as minor debt alongside the broader DRM mmap work (TD11).

**Where:** `kernel/src/mm/vma.rs` (`find_gap` + self-tests),
`kernel/src/proc/pcb.rs` (`reserve_unmapped_area`),
`kernel/src/syscall/handlers.rs` (window constants,
`alloc_user_mmap_reserve`, repurposed `mmap_alloc_vaddr`, `sys_mmap`
`reserved`-flag plumbing), `kernel/src/syscall/linux.rs`
(`linux_file_mmap` fixed/non-fixed atomic-reserve restructure +
`linux_file_mmap_fill` helper). Unblocks **known-issues.md TD9** (the
allocator dependency now exists; only the randomisation policy remains).

**How to reverse:** restore a single monotonic `AtomicU64` in
`mmap_alloc_vaddr` spanning the whole window and route the anon/file paths
back through a find-only helper + `add_vma`; drop `reserve_unmapped_area`
and the `reserved` flag. (Doing so re-introduces all three original defects
and the SMP race, so this is not advised.)
