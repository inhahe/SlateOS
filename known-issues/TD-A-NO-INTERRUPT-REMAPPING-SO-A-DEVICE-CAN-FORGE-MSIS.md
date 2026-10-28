## TD-A-NO-INTERRUPT-REMAPPING-SO-A-DEVICE-CAN-FORGE-MSIS (lane A, 2026-10-27) — OPEN

**Status:** OPEN -- fixed on lane-a-wip (76f772966, and the batch-22
follow-up), awaiting a boot on main. Every IOMMU unit remaps interrupts: a
device's MSI and each I/O APIC pin go through an entry naming the one
function allowed to raise it, and compatibility-format interrupts are
blocked. What QEMU cannot show -- the blocking itself, and a device's plain
DMA write into the MSI range, both below -- is for the test PC's boot.

**In short:** a device signals an MSI by writing a small message to a fixed
address range (`0xFEEx_xxxx`). The IOMMU's DMA remapping, which confines what
memory a userspace driver's device may reach (design-decisions 1579), does
not cover that range: those writes are interrupt requests, which only the
IOMMU's separate *interrupt remapping* checks. SlateOS leaves interrupt
remapping off. So a device driven from userspace -- or a malicious device --
can send any interrupt vector to any CPU, not just the one vector it was
given: it can fire another driver's interrupts, or the kernel's own
(the timer, the IPIs). Nothing breaks today; it is a hole in the sandbox the
device interface promises.

**Where:** `kernel/src/iommu_remap.rs` -- `quiesce_unit` clears the global
command's IRE bit and nothing sets it again; there is no interrupt remapping
table. `kernel/src/msi.rs` programs a device's MSI address and data directly
(compatibility format), which is what works with remapping off.

**The proper fix** (taken up by lane A 2026-10-28, in three phases, each
booted and published on its own):

0. **Queued invalidation first.** The interrupt-entry cache can only be
   invalidated through the invalidation queue (VT-d §6.5.2: there is no
   register-based IEC invalidation), and once the queue is enabled every
   invalidation must go through it -- the register-based context and IOTLB
   invalidations `iommu_remap` uses today included. So: a queue page per
   unit (IQA), QIE set, context-cache, IOTLB and wait descriptors, the
   register paths kept only for a unit without ECAP.QI, or one whose queue
   will not come on. **Done 2026-10-28** (`queue_run`): a refused descriptor
   is replaced by a wait and fails its run, a timed-out queue is left alone
   and fails every later run, and kexec drains and stops each queue.
- An interrupt remapping table (IRT) per IOMMU unit, its address given with
  SIRTP, and IRE set in the global command, with compatibility-format
  interrupts blocked (CFI clear) so that only remapped ones are delivered.
- `msi::enable_msi` then allocates an IRTE for the vector, with its source-id
  set to the device's bus:device.function so that only that device can raise
  it, and programs the device's MSI in remappable format (the handle of the
  IRTE, not the vector).
- The I/O APIC's lines go through IRTEs too once remapping is on.

  **Done 2026-10-28** (`iommu_remap::enable_interrupt_remapping`): one table
  every unit shares, all units or none -- a unit that cannot translate
  remaps too -- and only where the DMAR's INTR_REMAP flag allows it; the
  I/O APIC's requester id read from its DMAR scope (`iommu::ioapic_scope`),
  each pin's entry handle the pin's number (`ioapic::route`); an MSI's entry
  checks the requester id it really arrives with, a bridge's alias or bus
  range behind a PCIe-to-PCI bridge (`pci::msi_source`, Linux's
  `set_msi_sid`). The table's and the queues' locks are held with
  interrupts off, as Linux holds its own, since the IRQ balancer changes a
  pin from a softirq. `scripts/boot-test.sh` runs
  QEMU with `intremap=on` and fails a boot in which it never came on. QEMU
  does not emulate CFI -- it delivers a compatibility-format interrupt with
  remapping on whatever CFI says -- so in QEMU the blocking is checked only
  as GSTS.CFIS clear; the test PC is where a compatibility-format message is
  really refused.
- A test: a device's message carrying a vector not in its IRTE (a write by
  `edu`'s DMA engine to the MSI range) must not be delivered.

  **Done 2026-10-28** (`devhandle::forged_msi_test`): edu's MSI, pointed at
  an entry made for another function, is delivered on neither vector and is
  recorded as a source-id fault from edu (`iommu_remap::take_faults`). It
  is sent through edu's MSI capability rather than its DMA engine: QEMU
  gives a device's plain DMA writes no requester id (`pci_dma_write` passes
  `MEMTXATTRS_UNSPECIFIED`), so it never checks one that lands in the MSI
  range -- which a real unit does, on every write.

Found by the batch-16 review (Opus at max).
