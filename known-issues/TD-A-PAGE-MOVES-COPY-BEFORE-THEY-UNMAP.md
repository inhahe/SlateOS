## TD-A-PAGE-MOVES-COPY-BEFORE-THEY-UNMAP (lane A, 2026-10-03) — OPEN

**Status:** OPEN (lane A). Found while fixing
`TD-A-CROSS-PROCESS-COPY-RACES-THE-TARGETS-OWN-UNMAP`.

**In short:** when the kernel moves a process's page to another frame
(compaction, which packs memory together so large blocks can be allocated)
or out to swap (reclaim, under memory pressure), it copies the page first
and takes it out of the page table afterwards. A write the process makes in
between, from another of its threads on another CPU, goes into the old
frame after the copy was taken, and is lost: the process later reads its
page back without that write. Nothing reports it.

## Where

- `kernel/src/mm/compact.rs` `migrate_page`: copies the 16 KiB frame
  (step 2), then unmaps it and maps the new frame (step 3), then flushes the
  TLB (step 4). Run by `try_compact` when a higher-order allocation fails
  (`mm::frame`).
- `kernel/src/mm/swap.rs` `swap_out_page`: reads the frame into a buffer
  and writes the swap slot (steps 1-2), then unmaps it and flushes (steps
  3-5). Run by `try_reclaim`, from `kswapd` and from allocations that find
  memory short (`mm::frame`). Its safety contract asks that the page "not be
  actively accessed by another CPU/context during the swap-out operation",
  which no caller can promise for a running multi-threaded process.

A `process_vm_writev` into the page is lost the same way. Since 2026-10-03
the old frame is no longer reused while such a copy touches it
(`mm::frame`'s remote-copy windows), so the loss is the process's own data,
not another process's memory.

**To reproduce:** a process with two threads, one writing a counter into a
page in a loop, while memory pressure swaps that page out (or compaction
moves it). The counter goes back in time when the page returns.

## The proper fix

Take the page out of the mapping before copying it, and make anyone who
touches it meanwhile wait rather than see it missing. That is Linux's
migration entry: the PTE is replaced by a non-present entry that names the
migration, the TLB is flushed on every CPU, the copy is made, and a fault on
the entry waits for the migration to end and then retries. For swap-out the
swap entry can be written first, with a "write-back in progress" mark the
fault handler waits on.

- The fault handler (`pcb::try_resolve_fault`, `mm::fault`) must know the new
  entry kind; today a non-present PTE inside a VMA means "demand-page it",
  and would hand the thread a zero page.
- The remote-copy path (`mm::user::touch_remote_page`) resolves a missing
  page through that same resolver, so it inherits the wait.
- After the unmap and the flush, wait out remote-copy windows on the frame
  (`mm::frame::wait_for_remote_copies` is private today; it would need a
  `pub(crate)` door) before copying.
