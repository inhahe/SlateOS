## 1587. A device handle offers MSI-X, a vector per entry waited for together, and a Linux driver process uses it

**Date:** 2026-10-29 · **Decided by:** Claude (operator-approved scope) -- the operator chose userspace drivers through a device handle (§1579) and Linux's drivers through a layer (§1577); how interrupts reach them is Claude's call · **Lane:** A

**In short:** A device tells its driver it needs attention with an
interrupt. Until now a driver process could have one interrupt per device
(MSI), and Linux's e1000e could not run on QEMU's model of its card: given
no MSI-X, it falls back to MSI, tests it, and QEMU's model cannot pass that
test, so it falls back again to a line the layer does not route. Devices
now get MSI-X -- several interrupts per device, each with its own job
(received packets, sent packets, everything else) -- and e1000e on QEMU
passes packets with it.

### The decision

1. **The kernel's device handle offers MSI-X** (`kernel/src/msi.rs`,
   `kernel/src/devhandle.rs`): `SYS_PCI_DEVICE_IRQ_SETUP` with a vector
   count sets up the device's first MSI-X table entries, as many as asked,
   as its table holds and 32 allow, each sending a CPU vector of its own --
   with interrupts remapped, each naming a remapping entry of its own that
   only this function may raise. The kernel writes the table (mapped
   uncached in its own space) with the function masked meanwhile, turns MSI
   off first and the legacy line off after, as for MSI. A count of 0 is MSI
   as before.
2. **One device, one wait** (`msi::claim_group`): a device's vectors are
   claimed together, and an interrupt on any of them wakes the waiters of
   the first -- so `SYS_WAIT_MULTIPLE` on the handle, and
   `SYS_PCI_DEVICE_IRQ_WAIT`, wait for all of them, unchanged. The new
   `SYS_PCI_DEVICE_IRQ_PENDING` (1189) answers which vectors have had
   interrupts since last asked, as a mask, taking them: each vector's
   handler runs for its own. (`IRQ_PENDING_TAKE`; with `IRQ_PENDING_LOOK`
   it only looks, for a legacy line, §1591.)
3. **The table is the kernel's**: a mapping of the BAR that holds the
   table or its pending bits leaves their pages out (`map_bar`), as Linux's
   VFIO leaves them out -- a driver writing the table could aim its
   device's messages anywhere, interrupts not remapped -- and the MSI-X
   capability was already among the registers a driver may not write.
4. **A set-up replaces the last**: asked again for what it has, the
   device keeps it; asked for something else -- MSI after MSI-X, more
   vectors -- the old set-up goes first, its waiters woken to find it gone.
   So a driver can fall back from MSI-X to MSI, as Linux's drivers do. A
   refusal before anything changes (no such capability, a set-up in
   progress) leaves the old one; running out of vectors leaves none.
5. **The layer** (`linuxdrv/emul/lx_irq.c`): Linux's
   `pci_enable_msix_range()` and `pci_alloc_irq_vectors(PCI_IRQ_MSIX)` --
   for a function's first entries, in order, the ones the kernel programs
   -- with an interrupt number per vector (1024 + 32 x function +
   vector); each turn asks each function with a handler which vectors
   came, and runs each one's handler. An interrupt arriving while its
   handler is disabled is owed and run once it is enabled, as Linux replays
   a pending one; the kernel no longer holds it meanwhile, the mask having
   taken it.

### The alternatives

- **Every MSI-X entry sending one CPU vector**: no change to waiting, but
  the layer could not tell the vectors apart, so every handler would run
  for every interrupt -- work for nothing on every packet, and a driver
  meeting its handlers called for interrupts not theirs.
- **Route the legacy line instead**: e1000e on QEMU would fall back to it;
  but a legacy line is level-triggered and shared, needing the kernel to
  mask it until the driver process has run, and it is what every modern
  device avoids. MSI-X is what they use.
- **Leave MSI-X out and test e1000e on real hardware only**: the test
  PC's I219 has MSI only and passes e1000e's test; but the boot test, which
  is what proves a tree, would not run Linux's driver at all.

### What it costs

- A driver may name only its table's first entries, in order: Linux allows
  any, and none of the drivers planned names others. Asking for others is
  refused out loud (`pci_enable_msix_range`).
- At most 32 vectors per function, from a pool of 160 shared by the system.
- A register sharing a page with the table goes out of the driver's
  mapping with it: on the 82574 the table has a BAR of its own.

### How it is checked

`devhandle::self_test_msix`, on the boot test's spare 82574: three vectors,
the card's receive, transmit and other causes routed one to each, each
raised in turn reaching its own vector alone; the table and its pending
bits in no mapping of their BAR; the same set-up kept, MSI then replacing
it; all released at the close. Hello's "pci" run does the same through
Linux's API and the layer. And e1000e's run (§1588) uses three vectors to
pass packets.
