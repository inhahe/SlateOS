## 1579. A userspace driver reaches its device through one handle: owned, mapped, interrupted and DMA-sandboxed through it, and released with it

**Date:** 2026-10-27 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** SlateOS's design puts drivers in ordinary programs, but until
now a program could not actually drive a device: nothing let it own one,
reach its registers safely, get its interrupts the modern way, or give it
memory the device may read. The pieces that did exist trusted the program
with raw physical addresses, which was a hole. This entry decides the
interface instead. A driver opens a device and gets a handle -- like a file
handle -- and does everything through it: map the device's registers, read
and write its configuration, get its interrupts, and get memory the device
can reach. The kernel works out every address itself, so a driver can only
ever touch its own device. When the handle closes, or the driver crashes,
the kernel stops the device and takes everything back.

### The decision

1. **One handle per device.** `SYS_PCI_DEVICE_OPEN(bus:device.function)` returns
   a device handle, held like a native socket's (`ResourceType::PciDevice`
   in the process's held handles, never passed on by a fork). It needs a
   capability naming that function -- granted by
   whoever starts the driver (the service manager, or the boot test) -- and
   is refused while a kernel driver or another handle has the function.
2. **Everything goes through the handle.** No call takes a physical address.
   - *Registers:* `SYS_PCI_DEVICE_MAP_BAR(h, bar)` maps a memory BAR uncached
     (write-combining on request for a prefetchable one) and returns where;
     the kernel reads the BAR's address and size from the device itself.
   - *Configuration:* `SYS_PCI_DEVICE_CONFIG_READ/WRITE(h, offset, width)`, with
     the registers the kernel manages refused for writing -- the BARs, the
     expansion ROM, the MSI and MSI-X capabilities.
   - *Information:* vendor, device, class, revision, each BAR's kind and
     size, and the interrupt kinds the device supports.
3. **DMA is arenas in the device's own IOMMU domain.** Opening a device gives
   it a domain of its own (§1576). `SYS_PCI_DEVICE_DMA_ALLOC(h, size)` allocates
   frames, maps them into the process and into that domain, and returns the
   address the process sees and the one the device must be given. There is
   no call to map arbitrary process memory per transfer: the Linux layer
   serves every allocation a driver might hand a device from these arenas
   (as Genode does), so Linux's `dma_map_*` is arithmetic, not a system
   call per packet. Without an active IOMMU the device's address is the
   physical one and the device is not sandboxed; the kernel says so in the
   log when such a device is opened.
4. **Interrupts are events on the handle.**
   `SYS_PCI_DEVICE_IRQ_SETUP(h, kind, count)` sets up MSI-X, MSI or INTx vectors
   (the kernel programs the device) and returns how many it got. A fired
   vector sets its bit in the handle's pending mask and wakes waiters; the
   handle is ready in `SYS_WAIT_MULTIPLE` while any bit is set, so a driver
   waits on its device, its IPC channel and a timer in one call;
   `SYS_PCI_DEVICE_IRQ_TAKE(h)` returns the mask and clears it. INTx is
   level-triggered: when it fires the kernel quenches it at the device
   (the command register's INTx-disable bit) and `TAKE` re-arms it, so an
   unserviced device cannot storm.
5. **Closing releases everything, in the safe order.** On the handle's last
   close -- including when the process exits or crashes -- the kernel turns
   bus mastering off first, so the device stops reading and writing memory;
   then frees its interrupts, detaches and destroys its domain, frees its
   DMA frames, removes its register mappings, and returns the function
   unowned. A restarted driver opens it fresh.
6. **What it replaces.** The raw `SYS_DMA_*` domain calls, the line-number
   `SYS_IRQ_REGISTER/WAIT/RELEASE`, and `udriver.rs`'s bookkeeping (which no
   system call reaches) are superseded; they go once the device interface
   has its first user, since nothing uses them today.

### The alternatives

| Alternative | For | Against |
|---|---|---|
| **A device handle (chosen)** | One capability check at open, and every later call can only reach that device; release comes from the held-handle list every exit already empties; waiting joins the existing `SYS_WAIT_MULTIPLE`, which keys on the resource type | The exit order matters: a process's address space is freed when it becomes a zombie and its held handles only when it is reaped, so its devices are released at the zombie transition (`devhandle::release_owned_by`), before the teardown that would free their BARs as RAM |
| Extend `udriver.rs`'s driver ids | Code exists | Ids are not handles: nothing closes them on exit, nothing can wait on them, and every call would re-check ownership by id |
| Map any process memory for DMA per transfer | Any buffer can be given to a device | A system call and an IOMMU map and unmap per packet; pinning arbitrary process pages; the Linux layer does not need it |
| Interrupts as IPC messages on a channel | One wait mechanism | A message allocated per interrupt, a queue that can overflow under an interrupt storm; a pending mask coalesces, never allocates, never overflows |

### What it costs

- A driver's DMA memory is what it allocated as arenas up front; it cannot
  hand a device a buffer from anywhere else. The Linux layer is built for
  that; a native driver that needs streaming maps of process memory is the
  trigger for adding them.
- Without an IOMMU (some machines, or one switched off) a driver's device
  can still reach all of memory, as on every system without one.

### How it is checked

Kernel self-tests drive each call as a scratch process: open refused
without the capability and for a function already owned; a BAR mapped at
the address the device reports and nowhere else; configuration writes to
the managed registers refused; a DMA arena reachable through the domain;
and, after close, bus mastering off, the domain gone and the mappings gone.
The first real user is Linux's `e1000e` through the layer (§1578), on a
second QEMU e1000e in the boot test.
