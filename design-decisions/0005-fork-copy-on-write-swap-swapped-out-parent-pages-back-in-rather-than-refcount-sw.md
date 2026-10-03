## 5. fork() copy-on-write — swap swapped-out parent pages back IN rather than refcount swap slots

**Date:** 2026-05-31 (predates this file; recorded retroactively 2026-06-12)

**Decided by:** Claude (autonomous) — an implementation choice made while
building the CoW fork path.

**Context:**
`fork()` clones the parent address space copy-on-write. A parent page that
has been **evicted to swap** at fork time poses a question: the child must
end up sharing (CoW) the same logical page, but the page currently lives in a
swap slot, not in RAM. Either the swap slot becomes shared between parent and
child (requiring the swap subsystem to refcount slots), or the page is brought
back to RAM before the CoW share happens.

**Decision — bring the page back in first.**
`clone_user_half` (the CoW fork path) detects a PTE holding a swap entry and
calls `swap::swap_in_page(parent_pml4, virt, swap_in_default_flags())` to
fault the page back into RAM before CoW-sharing it. `swap_in_default_flags()`
returns `PRESENT | WRITABLE | USER_ACCESSIBLE | NO_EXECUTE`, mirroring the
page-fault handler's swap-in path (`idt.rs`), which likewise does not track
per-page protection and restores pages as user RW+NX. The page is then
re-registered as reclaimable so it can be evicted again later.

**Rationale:**
- Avoids adding a swap-slot refcount table and the associated free/evict
  bookkeeping (a slot shared by N address spaces can only be released when the
  last sharer drops it — that's a whole refcount lifecycle to get right).
- Keeps swap slots single-owner, which keeps the swap subsystem simple and its
  invariants easy to reason about.

**Cons / cost accepted:**
- A fork of a process with swapped-out pages pays I/O to page them back in,
  even if neither parent nor child ever touches them again. With swap-slot
  sharing, an untouched shared page would never need to come back.
- Transient RAM pressure: the swapped-out working set is materialized at fork.

**Alternatives considered:**
- *Refcount swap slots and share them directly across the CoW boundary* —
  rejected for now: materially more code and a new lifecycle to maintain; the
  swap-in approach is correct and simpler. **If fork-of-large-swapped-process
  becomes a measured hot path, switch to slot refcounting** — `clone_user_half`
  can then share the slot instead of calling `swap_in_page`.

**Where it lives:**
- `kernel/src/mm/cow.rs`: `clone_user_half` swap-entry branch (~line 557),
  `swap_in_default_flags()` (~line 365).
- `kernel/src/mm/swap.rs`: `swap_in_page`, `register_reclaimable`.

**How to reverse:**
- Add a refcount field to the swap-slot table; in `clone_user_half`, bump the
  slot refcount and copy the swap PTE into the child instead of swapping in;
  make swap-slot free refcount-aware; teach the fault handler that a faulting
  swap page may be shared (CoW-on-swap-in).
