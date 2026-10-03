## TD-A-CROSS-PROCESS-COPY-RACES-THE-TARGETS-OWN-UNMAP (lane A, 2026-10-02) — OPEN

**Status:** OPEN

**In short:** `process_vm_readv`/`writev` copy memory in or out of another
process, as a debugger does. Since 2026-10-02 the copy holds a pin
(`pcb::AsPin`, "a hold that keeps the target's page tables from being
freed"), so the target exiting or being reaped mid-copy no longer frees the
tables under it. But the target can still unmap a page of its own while the
copy is reading it. The copy would then touch a frame already back in the
free pool: a read returns whatever the frame's next owner wrote there, and a
write lands in the next owner's memory.

## What is wrong

- `syscall/linux.rs` `process_vm_impl` walks the target's tables through
  `mm::user::copy_from_user_as`/`copy_to_user_as`, page by page, holding
  only the pin.
- Nothing stops one of the target's threads from running `munmap`,
  `mremap`, `madvise(MADV_DONTNEED)` or an `exec` (which replaces the image)
  at the same moment. These clear the entry and free the frame while the
  copy holds the frame's address.
- Linux closes this with the target mm's `mmap_lock`, taken for reading
  around `pin_user_pages_remote`. The pages themselves are then pinned by
  reference count, so an unmap during the copy cannot free them.

**To reproduce:** a debugger reads a large range of a process while that
process unmaps the same range in a loop. The window is the length of one
page's copy, so this is a hammer-test finding, not an everyday one.

## The proper fix

Give each address space a reader-writer lock (or a page-reference scheme).
The unmap, remap and exec paths take it for writing. Cross-process copies
take it for reading, page by page as Linux's GUP does, or for the whole
copy. Points to watch:

- **Lock order:** taken by a syscall that may fault, so it ranks below
  nothing that a fault handler takes.
- **The copy faults on a missing page and answers `EFAULT` for it.** That
  has to stay true with the lock held: no demand-paging of the target's
  memory from another process.

## Where

- `kernel/src/syscall/linux.rs`: `process_vm_impl`.
- `kernel/src/mm/user.rs`: `copy_from_user_as`, `copy_to_user_as`.
- The unmap paths: `syscall/linux.rs` `sys_munmap`, `sys_madvise`,
  `sys_mremap`; `syscall/handlers.rs` `sys_munmap`; `proc/spawn.rs`
  `exec_process`.
