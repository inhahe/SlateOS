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
   (`SYS_PCI_DEVICE_DMA_ALLOC_AT`, 1188): a power of two of frames at an
   address inside anonymous memory the process reserved for itself
   (`SYS_MMAP` with `MAP_LAZY`) and has not touched. The arena is the same
   as `SYS_PCI_DEVICE_DMA_ALLOC`'s -- contiguous, in the device's IOMMU
   domain, freed with the device -- only placed. The kernel refuses a range
   outside such a reservation, so an arena takes nobody else's addresses.
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
   (`DMA_MAPPING_ERROR`), said once.
4. **Nothing to sync, no bounce buffers**: x86's DMA is cache-coherent and
   the arenas are mapped write-through, so every device is marked as
   needing no syncs (`dma_skip_sync`) and the syncs Linux still calls do
   nothing.
5. **The device's address width**: the window opens at a PCI device's
   default 32 bits; `dma_set_mask()` changes the width for the chunks
   backed after it (the kernel's widths: 64, 32, 24).

### The alternatives

- **The kernel's arenas where the kernel puts them** (`SYS_PCI_DEVICE_DMA_ALLOC`
  as it was): an arena lands high in the address space, aligned to its
  size, far from the heap, where Linux's direct map cannot cover it without
  a page array terabytes long; and `kmalloc`'s memory would still not be
  the device's.
- **A bounce buffer** (copy every buffer a driver maps into an arena):
  Linux's own SWIOTLB does this; it costs a copy per packet in both
  directions, which a network driver's performance cannot afford.
- **Map arbitrary process memory into the device's domain per mapping**:
  a system call and an IOMMU update per packet, and pinning (§1579 chose
  arenas for this reason).
- **Back the whole window at once**: simpler, but every driver process
  would commit 256 MiB of contiguous memory it mostly never uses.

### What it costs

- One device's window per process: a second function's device cannot
  reach the first's arenas (each has its own IOMMU domain), so its DMA
  mappings fail. Every driver planned so far drives one function.
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
