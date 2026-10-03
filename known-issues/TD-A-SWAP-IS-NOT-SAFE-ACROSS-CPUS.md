## TD-A-SWAP-IS-NOT-SAFE-ACROSS-CPUS (lane A, 2026-10-03) — OPEN

**Status:** OPEN (lane A). What is left of swap's problems after the
2026-10-03 fixes on lane-a-wip (below, "Fixed with this entry").

**In short:** swapping a page out and back in works on one CPU at a time
(since 2026-10-03: before that it did not work at all, see below), but
nothing stops two CPUs from swapping the same page in, or one swapping it
out while another touches it. Each can lose the page's data or corrupt the
swap slot accounting. Swap only runs under memory pressure, so this has not
been seen in a boot test; on a machine short of memory with a multi-threaded
program, it would be.

## What is wrong

- **Two CPUs swapping the same frame in.** Two threads of one process
  touching the same swapped page on two CPUs both run `mm::swap::swap_in_page`
  (from the #PF handler, which takes no lock). Both read the slot, both
  allocate a frame; one maps its frame and gives the slot back, the other
  finds the entries gone or the parts present. The loser's frame leaks, or
  worse, its rollback puts swap entries back over the winner's mapping.
- **Swap-out against a touch.** `swap_out_page` copies the frame before it
  replaces the entries (known-issues
  `TD-A-PAGE-MOVES-COPY-BEFORE-THEY-UNMAP`), so a write between the two is
  lost.
- **Disk swap in the fault handler.** The #PF handler runs on an interrupt
  gate, and `swap_in_page` reads the slot there; for the disk backend that
  is a block read with interrupts off.
- **A non-present entry is overwritten without a look.**
  `page_table::map_frame`/`map_frame_subpages` refuse only a *present*
  existing entry, so a mapping made over a swap entry drops it silently,
  leaking the slot and the data. The fault resolver now swaps in first, so
  its demand path no longer does this, but any other caller of `map_frame`
  over a swapped page would.
- **The reclaim list is a linear scan** (`register_reclaimable`,
  `unregister_reclaimable`: O(n) in the pages ever registered).

## The proper fix

A per-address-space lock for page-table changes that can block (Linux's
`mmap_lock`), held by swap-in, swap-out, munmap, mremap and exec, and by the
fault resolver around its decision -- with the #PF handler doing swap-in in
thread context, not on the interrupt gate. Then `map_frame` can refuse a swap
entry (`AlreadyExists`) once no legitimate caller maps over one. Linux also
keeps a swap cache, so a page being swapped in or out by one thread is found
by the next instead of being read twice.

## Fixed with this entry (lane-a-wip, 2026-10-03)

- **Swap-in never worked.** It looked the entry up with
  `page_table::read_leaf_pte`, whose doc said it returned non-present (swap)
  entries and whose code returned present entries only. `is_swapped` used it
  too, so it was false for every swapped page: the #PF handler went on to
  demand-page a page of zeros over the entry, and `fork` failed outright on
  a process with a page in swap. Every page swapped out came back as zeros.
- **The kernel faulting a page in zero-filled swapped pages.** The fault
  resolver (`pcb::resolve_fault`), which a syscall's buffer and
  `process_vm_readv` go through, did not look for swap at all.
- **Swap-in ignored the page's protection,** mapping every part writable
  and non-executable. Each part's flags are now kept in its swap entry.
- **Slots leaked** when a swapped page was unmapped or its process exited.
  A slot is now counted by the entries naming it and freed with the last.
- **No test.** `swap::test_round_trip_keeps_each_part` and
  `pcb::test_fault_swaps_in` now run at every boot.
