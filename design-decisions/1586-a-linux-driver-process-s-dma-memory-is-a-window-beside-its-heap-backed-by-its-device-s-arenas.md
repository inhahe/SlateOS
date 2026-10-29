## 1586. A Linux driver process's DMA memory is a window beside its heap, backed by its device's arenas a chunk at a time, and kmalloc serves from it

**Date:** 2026-10-29 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers through a layer (§1577); how the layer gives a device memory is Claude's call, under §1578 and §1579 · **Lane:** A

**In short:** A network card reads the packets it sends, and writes the
ones it receives, straight from and into memory ("DMA"). In SlateOS a
device may only reach memory the kernel has given it (§1579), but a Linux
driver hands its card ordinary memory it allocated with `kmalloc`. So the
layer arranges for that ordinary memory to be the device's: once a
device is given to a driver process, the process's allocations come from
a region the kernel backs with memory the device can reach, and telling
the card where a buffer is becomes simple arithmetic.

### The decision

1. **A new kernel call places a device's arena where the process says**
   (`SYS_PCI_DEVICE_DMA_ALLOC_AT`, 1188): a power of two of frames, on a
   frame boundary, inside one region of private anonymous memory the
   process reserved for itself (`SYS_MMAP` with `MAP_LAZY`) and has not
   touched there. The arena is the same as `SYS_PCI_DEVICE_DMA_ALLOC`'s --
   contiguous, in the device's IOMMU domain, freed with the device -- only
   placed. The kernel refuses a range outside such a reservation, so an
   arena takes nobody else's addresses, and checks everything before it
   allocates. While the arena lives, its range is a region of its own, as a
   mapped BAR's is -- never executable, not given to a forked child -- so
   that no path for ordinary memory (a page fault, a fork, swapping) ever
   meets the device's frames; freed, the range is the reservation's again.
2. **The DMA window** (`linuxdrv/kit`, `window.rs`, `sys.rs`): the kit
   reserves 256 MiB beside its 256 MiB heap, in the same region, so that
   Linux's direct map (§1584) covers both. When the process's first PCI
   function is opened, the window is opened for it, and from then on
   `kmalloc` serves from the window while it can hold the block. The
   window's allocator starts empty and is given each chunk (4 MiB) once the
   chunk is backed -- an allocator keeps its free lists in free memory, so
   it must never touch memory not yet backed -- and chunks are backed in
   order, as allocations need them.
3. **A DMA address is arithmetic** (`linuxdrv/emul/lx_dma.c`): each chunk
   is one arena, contiguous where the device sees it, so the bytes inside
   one chunk have one device address, the chunk's plus the offset.
   `dma_map_page()`/`dma_map_single()` answer it; `dma_alloc_coherent()`
   allocates from the window alone, zeroed, at least page-aligned. Memory
   outside the window -- allocated before the device came, the process's
   own data, a task's stack -- or a range crossing two chunks has no DMA
   address, and mapping it fails as an IOMMU's refusal would
   (`DMA_MAPPING_ERROR`), said once. So does all DMA for any device but
   the window's -- the first function given -- whose own IOMMU domain
   holds none of the window's arenas.
4. **Nothing to sync, no bounce buffers**: x86's DMA is cache-coherent and
   the arenas are mapped write-back, as the kernel's own view of their
   frames is (two views of a frame with different memory types are what
   the SDM forbids), so every device is marked as needing no syncs
   (`dma_skip_sync`) and the syncs Linux still calls do nothing.
5. **The device's address width**: the window opens at a PCI device's
   default 32 bits. The window serves both streaming mappings and coherent
   memory, so the chunks backed from then on are kept within the narrower
   of the device's two masks (`dma_set_mask()`, `dma_set_coherent_mask()`),
   at the widest of the kernel's widths inside it (64, 32, 24). A mask
   narrower than a chunk already backed is refused (`-EIO`), as Linux
   refuses a mask its platform cannot meet: `kmalloc` already serves from
   that chunk.

### The alternatives

- **The kernel's arenas where the kernel puts them** (`SYS_PCI_DEVICE_DMA_ALLOC`
  as it was): an arena lands high in the address space, aligned to its
  size, far from the heap, where Linux's direct map cannot cover it without
  a page array terabytes long; and `kmalloc`'s memory would still not be
  the device's.
- **A bounce buffer** (copy every buffer a driver maps into an arena):
  Linux's own SWIOTLB does this, for devices it does not trust and in
  confidential virtual machines. It keeps the driver's own memory out of
  the device's reach (see "What it costs"), at a copy per packet in each
  direction: about 2% of a core at 1 Gb/s, about 20% at 10 Gb/s. Whether
  some devices should pay it is the operator's question A-Q30.
- **Map arbitrary process memory into the device's domain per mapping**:
  a system call and an IOMMU update per packet, and pinning (§1579 chose
  arenas for this reason).
- **Back the whole window at once**: simpler, but every driver process
  would commit 256 MiB of contiguous memory it mostly never uses.

### What it costs

- **The device reaches all of its driver's `kmalloc` memory**, not only
  the buffers it is handed: the driver's own records, timers and the
  pointers to its code included, for as long as the process lives. A
  working device does nothing with that; a broken or hostile one could
  overwrite them and take over its driver process -- never the kernel or
  another process, which its IOMMU domain keeps it from. Linux's own
  exposure for a trusted device is smaller (the pages around a buffer,
  while it is mapped). Open question A-Q30 asks whether some devices --
  external ones, as Linux does -- should get bounce buffers instead.
- One device's window per process: a second function's device cannot
  reach the first's arenas (each has its own IOMMU domain), so all its DMA
  is refused. Every driver planned so far drives one function.
- A device whose mask is narrower than the window's first chunk (a 24- to
  31-bit device; the chunk is backed below 4 GiB before probing) has its
  mask refused, and its driver fails to probe.
- A DMA buffer is at most a chunk (4 MiB); larger coherent allocations
  fail.
- A driver process given a device commits at least one chunk (4 MiB) of
  contiguous memory once it allocates.
- The kernel's `SYS_MMAP` reservation it fills must stay untouched until
  backed; the window's allocator guarantees that for the window.

### Where it lands

`kernel/src/mm/dma.rs` (`alloc_for_user_at`, the shared placement),
`kernel/src/devhandle.rs` (`dma_alloc_at`, the reservation check, the
shared hand-over to the device), the syscall (`number.rs`, `handlers.rs`,
`dispatch.rs`, and its dispatch self-test on QEMU's `edu`),
`linuxdrv/kit/src/{window,buddy,sys}.rs`, `linuxdrv/emul/lx_dma.c`,
`linuxdrv/emul/lx_pci.c` (the window opened for the first function), and
hello's "pci" run (the card reading a descriptor and a frame from the
window and writing the descriptor's status back).
