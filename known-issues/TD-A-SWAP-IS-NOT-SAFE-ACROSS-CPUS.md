## TD-A-SWAP-IS-NOT-SAFE-ACROSS-CPUS (lane A, 2026-10-03) — OPEN

**Status:** OPEN (lane A). What is left of swap's and the fault path's
cross-CPU problems after the 2026-10-03 fixes on lane-a-wip (below, "Fixed
with this entry").

**In short:** swapping a page out and back in works (since 2026-10-03:
before that it did not work at all), and two CPUs resolving the same page
now install once. What is left is the case the page-table lock cannot cover
because it must not be held across a file or disk read: an unmap racing a
fault on the same range, and a page being moved while it is written. Swap
and compaction only run under memory pressure, so none of this has been
seen in a boot test.

## What is wrong

- **munmap against a fault.** `munmap` clears the entries
  (`mm::user::unmap_user_range`) and then removes the VMA
  (`pcb::remove_vma_range`). A fault between the two finds the VMA still
  there and populates the range being unmapped: the frame stays mapped with
  no VMA, and leaks until teardown. Linux holds `mmap_lock` across the
  whole fault, slow half included, and munmap takes it for writing.
- **Two first faults on one straddling frame** (a 16 KiB frame whose 4 KiB
  parts belong to two VMAs: `pcb::resolve_subpaged_fault`) can each
  allocate a frame and back different parts with different frames. The
  resolver reads the file between its maps, so it cannot hold the
  page-table lock throughout.
- **Swap-out against a touch.** `swap_out_page` copies the frame before it
  replaces the entries (known-issues
  `TD-A-PAGE-MOVES-COPY-BEFORE-THEY-UNMAP`), so a write between the two is
  lost. It now refuses if the entries changed meanwhile, which catches a
  first write (the dirty bit) but not later ones.
- **Disk swap in the fault handler.** Swap-in reads the slot from the #PF
  handler; for the disk backend that is a block read in exception context
  (interrupts are on for a user-mode fault, but whether a block read may
  block there is unexamined).
- **A non-present entry is overwritten without a look.**
  `page_table::map_frame`/`map_frame_subpages` refuse only a *present*
  existing entry, so a mapping made over a swap entry drops it silently,
  leaking the slot and the data. The fault resolver swaps in first, so its
  demand path no longer does this, but any other caller of `map_frame` over
  a swapped page would.
- **The reclaim list is a linear scan** (`register_reclaimable`,
  `unregister_reclaimable`: O(n) in the pages ever registered).

## The proper fix

A sleepable per-address-space lock (Linux's `mmap_lock`), held for reading
across a fault's slow half and for writing by munmap, mremap, exec and fork,
with the #PF handler doing its slow half in a context that may sleep. The
page-table lock (`mm::as_lock`) stays for the installs. Then `map_frame` can
refuse a swap entry (`AlreadyExists`) once no legitimate caller maps over
one, and the straddling-frame resolver can hold the sleepable lock across
its reads. Linux also keeps a swap cache, so a page being swapped in or out
by one thread is found by the next instead of being read twice.

## Fixed with this entry (lane-a-wip, 2026-10-03)

- **Swap-in never worked.** It looked the entry up with
  `page_table::read_leaf_pte`, whose doc said it returned non-present
  (swap) entries and whose code returned present entries only. `is_swapped`
  used it too, so it was false for every swapped page: the #PF handler went
  on to demand-page a page of zeros over the entry, and `fork` failed
  outright on a process with a page in swap. Every page swapped out came
  back as zeros.
- **The kernel faulting a page in zero-filled swapped pages.** The fault
  resolver (`pcb::resolve_fault`), which a syscall's buffer and
  `process_vm_readv` go through, did not look for swap at all.
- **Swap-in ignored the page's protection,** mapping every part writable
  and non-executable. Each part's flags are now kept in its swap entry.
- **Slots leaked** when a swapped page was unmapped or its process exited.
  A slot is now counted by the entries naming it and freed with the last.
- **Two CPUs resolving the same page** (a copy-on-write break, a demand
  page, a swap-in) each installed: lost writes, a frame's reference dropped
  twice (a use-after-free with two children sharing it), and SIGSEGV for
  the second thread's valid access. Installs now run under the page-table
  lock and check the entries first (`mm::as_lock`).
- **No test.** `swap::test_round_trip_keeps_each_part`,
  `swap::test_swap_in_twice`, `cow::test_cow_break_twice` and
  `pcb::test_fault_swaps_in` now run at every boot.
