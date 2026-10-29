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

   *As built (2026-10-27): MSI, one vector.* `SYS_PCI_DEVICE_IRQ_SETUP(h)`
   (1186) programs the device's MSI capability; `SYS_PCI_DEVICE_IRQ_WAIT(h,
   block)` (1187) takes the count, at once or after blocking until there is
   one -- woken from the interrupt, and by the handle's close. Several
   threads may wait; every interrupt wakes them all, and whoever takes the
   count has it.

   *As built (2026-10-28): readiness in `SYS_WAIT_MULTIPLE`.* A device
   handle is a kind there (`ResourceType::PciDevice`): `POLLIN` while its
   interrupts are counted and not yet taken, `POLLHUP` once it is closed,
   `POLLERR` while its MSI is not set up -- set it up before waiting on the
   device. So a driver's one thread waits on its device, its channels and
   its timers together (§1578), and takes the interrupts with `IRQ_WAIT`'s
   non-blocking form. Each vector keeps a waiter set the interrupt handler
   try-locks and wakes, a wake it cannot make being owed to the next tick;
   the wait resolves the device's vector once and registers, tests and lets
   go on that one vector. MSI-X, INTx and several vectors are still to
   come.

   *As built (2026-10-28): the MSI is remapped.* Where the IOMMU remaps
   interrupts (`iommu_remap::interrupts_remapped`), the device's MSI names
   an interrupt remapping entry of its own (`alloc_msi_entry`), which only
   that function's requester id may raise, instead of carrying a vector and
   a CPU itself; compatibility-format messages are blocked. So a driver --
   or its device -- can no longer raise another device's interrupt or the
   kernel's own by writing to the MSI address range.

   *As built (2026-10-29): INTx.* `SYS_PCI_DEVICE_IRQ_SETUP(h, 0, 1)`
   (`arg2` `IRQ_SETUP_INTX`; 0 is MSI and MSI-X, as before) sets up the
   function's legacy line, as the firmware routed it (the Interrupt Line
   register, as the kernel's own PCI drivers take it -- not ACPI's `_PRT`:
   known-issues `A-PCI-INTX-ROUTING-IS-THE-INTERRUPT-LINE-REGISTER-NOT-ACPI-PRT`).
   Refused, `NotSupported`, for a function with no pin, a line the I/O
   APIC lacks or one the kernel's ISA devices use, a function that cannot
   mask its INTx (before PCI 2.3), or a line the I/O APIC would not
   unmask. The set-up turns off any MSI or MSI-X the function was left
   with, makes the line level-triggered and active-low, and leaves a line
   the storm detector masked for it to unmask. Its interrupts are
   delivered through an MSI pool vector the
   handle claims and no hardware raises, so everything above -- the count,
   the mask, waiting here and in `SYS_WAIT_MULTIPLE` -- is MSI's: the I/O
   APIC line's handler (`devhandle::handle_intx`, from
   `ioapic::handle_device_irq`) finds each such function asserting with its
   INTx armed, sets its INTx-disable bit, and counts the interrupt on that
   vector; a take that finds interrupts clears the bit. A function its
   driver has not serviced by then interrupts once more and is quenched
   again: it cannot hold a shared line. The bit is the kernel's from open
   to release -- set at open, and set again by every tear-down (turning
   MSI off turns the legacy line back on), cleared only while INTx is set
   up and armed -- and a driver's write to the command register keeps it
   as it is. Every change the kernel makes to the command register, the
   handler's quench included, is one read and one write under one hold of
   the configuration ports (`pci::update_command`), and a driver's
   configuration writes are made under the device table's lock, so none
   undoes another's bits or lands after a release. The line stays unmasked at the I/O APIC
   afterwards: a kernel driver may share it, and a line no function asserts
   is quiet. The handler finds its functions in a lock-free table of 16
   slots, published and unpublished around the handler's own count of
   itself, so none delivers on a vector once it is freed.
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
| **A device handle (chosen)** | One capability check at open, and every later call can only reach that device; release comes from the held-handle list every exit already empties; waiting joins the existing `SYS_WAIT_MULTIPLE`, which keys on the resource type | The exit order matters: a device must be released while its owner's page table still exists, which `pcb::exit_close_fds` (held handles) running before `release_address_space` gives; and the generic memory paths must not free or copy its pages (`PageFlags::FOREIGN`) |
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
INTx is checked on QEMU's `edu` with its MSI off (`self_test_intx`): each
interrupt quenched and counted once, edu still asserting until
acknowledged; a take re-arming it; one taken unserviced interrupting once
more and no more; a parked waiter woken; the INTx-disable bit kept through
the driver's command-register writes, both ways; MSI replacing it and the
close leaving it quenched, its slot and vector free.
The first real user is Linux's `e1000e` through the layer (§1578), on a
second QEMU e1000e in the boot test.
