### [A] A-FORK-MADE-SHARED-MEMORY-COPY-ON-WRITE: after a fork, the parent's next write to a shared-memory region, an io ring, a DMA or display buffer, or device memory went to a private copy -- 2026-09-27
**Status:** FIXED on lane-a 2026-09-27, awaiting a boot. Found designing
process-shared futexes (lane D's
`d-a-futexes-keyed-by-physical-page-for-process-shared-objects`).

**In short:** some memory is meant to be seen by more than one party at once:
a shared-memory region two programs use to talk, the ring a program shares
with the kernel, a buffer a device reads, the screen's image. When a program
that had such memory mapped started a child with `fork`, the kernel quietly
turned that memory into a private copy for whichever side wrote to it first.
The program went on writing, and nobody else saw it -- no error, the sharing
simply stopped. For device memory it was worse: a write copied the device's
registers into ordinary RAM.

**Where.** `kernel/src/mm/cow.rs` `clone_frame_group` made every writable user
page copy-on-write in parent and child. The region, the kernel's ring, the
device or the display engine each hold their own reference to the frame, so
the copy-on-write fault that followed always copied. Fork also recorded a
reverse mapping for every frame it shared into the child, these included,
while the parent's own mapping of them had none. With exactly one mapper,
`rmap::is_private` called the frame private, and compaction could migrate it:
the same disconnection, done by the compactor.

**The fix.** A software page-table bit, `PageFlags::SHARED` (bit 10, one of
the three the hardware leaves to the OS; bit 9 is `COW`), set at the five
places the kernel maps shared-by-design memory into a process:
- `SYS_SHM_MAP`;
- the io ring's setup;
- `SYS_MMAP` with `MAP_MMIO`;
- the DRM dumb-buffer map;
- `mm::dma::alloc_for_user`.
Fork maps such a page into the child as it is, same frame and permissions,
leaves the parent's entry writable, and records no reverse mapping for it.
Compaction's `migrate_page` refuses one. `mark_cow` refuses one. The bit is
also what process-shared futexes key on (see that commit).

**Tests.** `mm::cow::test_fork_keeps_shared_pages_shared` forks a synthetic
address space holding a shared page. Both sides must map the one frame
writable, with no `COW` and no rmap entry, and the reference counts must
balance through teardown. `mark_cow` must refuse the page.

**Shared anonymous memory, done 2026-10-01.** Native `SYS_MMAP` takes
`MAP_SHARED` (`1 << 7`): committed pages marked `SHARED`, so a fork shares
them. `MAP_SHARED | MAP_LAZY` is refused, since a lazily faulted page would
not be shared. The Linux `MAP_SHARED | MAP_ANONYMOUS` maps the same way
(it was `ENOSYS`), and a map with neither type is `EINVAL`. Test:
`syscall::dispatch::test_dispatch_shared_anonymous_memory`. libc's half --
it passes `prot` through as the native flags and never says `MAP_SHARED` --
is `requests/a-d-libc-mmap-should-say-map-shared-and-translate-prot.md`.

**Still not done:** a writable `MAP_SHARED` of a *file* is `ENOSYS`. There is
no writeback, so a write could not reach the file.
