## TD-A-CROSS-PROCESS-COPY-RACES-THE-TARGETS-OWN-UNMAP (lane A, 2026-10-02) — OPEN

**Status:** OPEN -- fixed on lane-a-wip 2026-10-03 (the commit that adds
`mm::frame::RemoteCopyWindow`), awaiting a boot on main. What it leaves is
in "Not covered" below, with its own entry.

**In short:** `process_vm_readv`/`writev` copy memory in or out of another
process, as a debugger does. Since 2026-10-02 the copy held a pin
(`pcb::AsPin`, "a hold that keeps the target's page tables from being
freed"), so the target exiting or being reaped mid-copy no longer freed the
tables under it. But the target could still unmap a page of its own while
the copy was reading it, and the copy would then touch a frame already back
in the free pool: a read returned whatever the frame's next owner wrote
there, and a write landed in the next owner's memory. An exec in the target
was worse: it frees every page table under the PML4 it keeps, so the copy
could walk freed tables.

## What was wrong

- `syscall/linux.rs` `process_vm_impl` walks the target's tables through
  `mm::user::copy_from_user_as`/`copy_to_user_as`, page by page, holding
  only the pin.
- Nothing stopped one of the target's threads from running `munmap`,
  `mremap`, `madvise(MADV_DONTNEED)` or a copy-on-write break at the same
  moment. These clear the entry and free the frame while the copy holds the
  frame's address.
- `exec` (`proc::spawn::exec_process`) clears the user half in place
  (`page_table::clear_user_address_space`), freeing the tables as well as
  the frames, and the pin only kept the PML4 page itself.

## The fix

Three parts, none of them a lock that the target's own paths take:

1. **A frame cannot be freed while a copy touches it** (`mm::frame`,
   "Frames a cross-address-space copy is touching"). Each page's touch is
   a window with interrupts off (`RemoteCopyWindow`): the copier announces
   the 4 KiB page in its CPU's slot, walks the mapping again, and touches
   the page only if it is still there (`mm::user::touch_remote_page`).
   `free_frame` and `free_order` first wait out any window that announced
   a page inside what they free (`wait_for_remote_copies`). It is the
   store-buffering pattern with SeqCst on both sides: either the free sees
   the announcement and waits, or the copier's second walk sees the page
   gone and touches nothing. A window is one walk and at most one 4 KiB
   copy, uninterruptible and lock-free, so the wait is short and cannot
   deadlock. A free pays one fence and one load when no window is open.
   Linux pins the page by reference under the target mm's `mmap_lock`
   instead.
2. **fork waits for writes in flight** (`mm::cow::clone_address_space_cow`
   calls `wait_for_open_remote_copies`): a remote write that passed its
   checks before the parent's page turned copy-on-write is writing the
   frame the child now shares, and it ends before the child can run and
   copy it.
3. **exec waits for the pins** (`pcb::begin_exec_teardown`): its in-place
   teardown begins only when no pin holds the space, and no pin is given
   out until it ends (`pcb::ExecTeardown`); a pin asked for meanwhile waits
   and then sees the new image. Linux gives the new image a new mm instead.

Tests: `frame::test_remote_copy_window` (what a free waits on, and that a
closed window holds nothing) and `pcb::test_exec_teardown_holds_out_pins`
(the teardown's state machine). The race itself takes two CPUs and a hammer
loop and is not driven at boot.

## Not covered

- **A remote write can still be lost to a page that moves.** Compaction
  (`mm::compact::migrate_page`) and swap-out (`mm::swap::swap_out_page`)
  copy a page *before* they unmap it, so a write made in between, by a
  remote copy or by the owner's own threads, lands in the old frame and is
  lost. The frame is not reused under the copy any more, so this is lost
  data, not someone else's memory. Tracked as
  `TD-A-PAGE-MOVES-COPY-BEFORE-THEY-UNMAP`.
- **A frame reused without being freed.** The wait is in the two free
  paths. Anything that hands a user frame to another owner without
  freeing it would get round it; none is known.
