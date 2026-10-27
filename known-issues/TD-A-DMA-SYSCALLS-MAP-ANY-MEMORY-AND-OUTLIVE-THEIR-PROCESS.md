## TD-A-DMA-SYSCALLS-MAP-ANY-MEMORY-AND-OUTLIVE-THEIR-PROCESS (lane A, 2026-10-27) — OPEN

**Status:** OPEN.

**In short:** the system calls a userspace driver uses to give its device a
private view of memory (an IOMMU "domain": the list of memory a device may
reach) have two gaps. A driver may put *any* physical memory into its
device's view -- the kernel's, another program's -- because nothing checks
that the memory is the driver's own. And a domain is never taken down when
the driver that made it exits, so its id and page tables are lost for the
rest of the boot. Nothing uses these calls yet (no driver sandboxes its
device), so nothing goes wrong today; the first driver that does would be
able to read or overwrite all of memory through its device.

**Where:** `kernel/src/syscall/handlers.rs`, `sys_dma_map` (takes a raw
`phys_addr` behind only a `PortIo` capability) and the other `sys_dma_*`;
`kernel/src/iommu_remap.rs`, `map_dma_as`; nothing in process teardown
destroys a process's domains (`udriver.rs` says as much: "In a full
implementation: iommu_remap::destroy_domain()"). Since 2026-10-27 a domain is
at least its creator's alone (`iommu_remap::Caller`): another process's call
on it is refused.

**The proper fix:**
- `sys_dma_map` maps only memory the calling process owns: the frames
  behind a range of its own address space (as `mm::dma`'s allocations are),
  pinned for as long as they are mapped, not a physical address it names.
- A process's exit detaches its devices and destroys its domains, as it
  closes its handles (a hook in process teardown beside the handle table's).
- Then the `PortIo` capability check can become one for the device itself:
  attach only a function the process holds a capability for.
