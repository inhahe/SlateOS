## 1584. A Linux driver process's PCI functions are Linux's own PCI core's, through the device handle; its heap is Linux's direct map

**Date:** 2026-10-29 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers through a layer (§1577); how the layer presents a device is Claude's call, under §1578 and §1579 · **Lane:** A

**In short:** A Linux driver finds its hardware through Linux's PCI code,
which normally scans the machine's buses itself. In a driver process the
kernel owns the buses, and hands the process only the one device it may
drive. The layer now feeds Linux's own PCI code that one device, through
the "device handle" the kernel gives the process (§1579). Linux then finds
the device, sizes its memory windows, matches it with the driver and
starts the driver, exactly as on Linux, while every access still goes
through the kernel's checks. Separately, Linux code turns addresses of its
memory into page records and back by plain arithmetic; the process's
memory is now laid out so that arithmetic holds.

### The decision

1. **The process is told its functions on its command line** --
   `pci=BB:DD.F` per function, in hex -- and holds a capability for each.
   The kernel's self-test gives the boot test's spare 82574 so.
2. **Linux's own PCI core** (`drivers/pci`: access, bus, probe, host bridge,
   remove, pci, pci-driver, search, setup-res, setup-bus, iomap, quirks,
   pci-sysfs, pci-label, vpd, ASPM, vc, rebar, slot, proc, irq, and the MSI
   capabilities' init; `kernel/resource.c`; `lib/iomap.c`) is in every
   driver process (`linuxdrv/emul/manifest`).
3. **The layer is its host bridge** (`linuxdrv/emul/lx_pci.c`): at
   subsystem level, where x86 Linux scans (`pci_subsys_init`), it opens each
   function's handle and makes a root bus of its number with Linux's own
   `pci_create_root_bus()`, the memory space and the one bus number its
   windows, the configuration accessors the handle's; then scans only the
   functions given (`pci_scan_single_device()`). A function the process was
   not given reads as absent, and a slot's function 0 need not be given:
   Linux's own slot scan would stop at it (the I219 e1000e drives on a PCH
   is 00:1f.6). Linux's probe then reads the
   function, claims its BARs in Linux's own resource tree, and the driver
   core matches it with the driver when that registers.
4. **The BARs are shadowed** in the system-facing half
   (`linuxdrv/kit/src/bars.rs`, host-tested), because the handle refuses
   writes to them (the kernel sized and placed them): reads answer as the hardware would
   after what Linux wrote -- a memory BAR's address bits below its size read
   0, its type bits are the device's -- so Linux's own probe sizes each BAR
   by writing all ones, as it does on hardware, and the BAR never moves. An
   I/O BAR and the expansion ROM read as absent: the handle offers neither.
   Any other refused write (the MSI capabilities, the power state, a
   function reset) fails as the hardware's own failure would.
5. **`ioremap()` is the handle's BAR mapping**: the BAR that holds the
   address, mapped whole and uncached, once. A BAR Linux moved, or memory no
   BAR of the process's holds, is refused. Write-combining is uncached
   here, and x86's page attribute table reads as off.
6. **A function no driver took ends the process** (`SLX_EXIT_INIT_FAILED`),
   once the initcalls and any deferred probing are done: a driver process
   exists to drive what it was given, and a supervisor must see a failed
   probe as a failure, not a quiet exit.
7. **The process's heap is Linux's direct map** (`linuxdrv/emul/lx_mm.c`):
   `page_offset_base` is the heap's base, so an address's "physical"
   address is its offset into the heap, and `vmemmap_base` is a page array
   laid beside it, reading 0 and backed only where touched. Linux's own
   `__pa()`, `virt_to_page()` and `page_address()` then hold for every block
   kmalloc hands out, and kmalloc marks its blocks' pages slab, as Linux's
   own slab does (`kernel/resource.c` frees a released resource by it).
8. **Interrupt routing is not the PCI core's**: a device's legacy line is
   not routed (`pcibios_alloc_irq`), no MSI interrupt domain exists, and
   bus numbers are never reassigned. Interrupts come next, as the MSI the
   kernel programs through the handle.

### The alternatives

- **The layer's own `struct pci_dev`** filled in by hand from the handle's
  answers, and its own `pci_register_driver()` matching by id table:
  smaller, but every helper a driver calls -- capability walks, power
  states, `pci_enable_device`, resource requests, ASPM, save and restore --
  would read fields the layer had to know to fill, and drift from Linux.
- **Let the kernel answer BAR sizing** (pass the all-ones write through and
  have the kernel fake the read-back): it moves an emulation into the
  kernel that only the Linux layer needs.
- **A separate memory map for page arithmetic** (a table from heap block to
  page record) instead of the direct map: Linux's own inline arithmetic
  could not use it; every helper that turns an address into a page would
  need replacing.

### What it costs

- Every driver process carries the PCI core: hello grew from about 407 KB
  to about 661 KB.
- Configuration space past 256 bytes (PCIe extended capabilities: AER,
  SR-IOV) is invisible: the handle reaches only the first 256.
- An address outside the heap -- the process's own data, a task's stack --
  has no page; a driver that DMAs from a static buffer would break. Linux
  forbids that too (`CONFIG_DEBUG_VIRTUAL`).
- The page array costs 64 bytes for every 4 KiB of the heap that is
  handed out.
- Every driver process logs one failed subsystem initcall, carried past:
  `kernel/resource.c`'s `iomem` file system, through which Linux revokes
  `/dev/mem` mappings of a region a driver claims, cannot be mounted
  ("Cannot mount iomem pseudo filesystem: -19") -- there is no file system
  to mount it in, and no `/dev/mem` to revoke.

### Where it lands

`linuxdrv/emul/lx_pci.c` (the host bridge, `ioremap`, the bound check,
x86's PCI hooks), `linuxdrv/emul/lx_mm.c` (the direct map),
`linuxdrv/emul/lx_fs.c` (procfs and the `iomem` mount answered none),
`linuxdrv/kit/src/pci.rs` and `bars.rs` (the handle's system calls, the
shadowed BARs),
`linuxdrv/link.ld` (the PCI fixup tables), `kernel/src/linuxdrv.rs` (the
two runs given the card), and hello's "pci" and "pcifail" modes.
