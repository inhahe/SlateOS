## TD-A-NO-INTERRUPT-REMAPPING-SO-A-DEVICE-CAN-FORGE-MSIS (lane A, 2026-10-27) — OPEN

**Status:** OPEN.

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
- A test: a device's message carrying a vector not in its IRTE (a write by
  `edu`'s DMA engine to the MSI range) must not be delivered.

Found by the batch-16 review (Opus at max).
