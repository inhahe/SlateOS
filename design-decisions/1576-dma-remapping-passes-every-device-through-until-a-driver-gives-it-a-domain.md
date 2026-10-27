## 1576. DMA remapping passes every device through until a driver gives it a domain

**Date:** 2026-10-27 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** an IOMMU (a chip that checks which memory each device may
read or write on its own) can isolate a driver's device, but only once it
is told what each device may reach. SlateOS turned it on with nothing in
its tables, which blocks every device: on the first real PC it was tried
on, the kernel stopped at boot. Now every device keeps working as if there
were no IOMMU, and only a device a driver asks to have sandboxed is held to
the memory its driver maps for it.

### What was wrong

`iommu_remap::init` pointed each Intel VT-d unit at an empty root table and
turned translation on. Every device's DMA then faults: the display's
scan-out, USB, the disks. It had never run: QEMU, where every boot test
runs, had no IOMMU. On the test PC (a Z370 board, VT-d on) the kernel
stopped earlier still, on its first read of a unit's registers, which
nothing had mapped. Its context entries put the domain id and address
width in the wrong bits, its global commands carried one-shot status bits
back as commands, and an IOTLB flush was a context-cache flush, which does
not flush the IOTLB.

### The design

- A unit is turned on only if it can pass a device through untranslated
  (ECAP.PT) and its deepest table is 4-level, the depth a domain's table
  is and a pass-through entry must name. Any other unit is left off, its
  devices unaffected, and the log says why.
- One root table, shared by every unit turned on: all 256 buses, every
  device and function on each passing through (domain id 1).
- `attach_device` gives a device a domain's translation; `detach_device`
  passes it through again, as every device the kernel drives itself does.
  A context entry is changed by making it not present, invalidating, then
  writing it whole, so that no unit reads half of each.
- Where a unit does not snoop the CPU caches when it walks the tables
  (ECAP.C clear, as Intel's graphics unit often is), every table write is
  written back with `clflush` before the unit is told to look.
- Context and IOTLB invalidation per the spec, drained where the unit can;
  their failures are returned.

### The alternatives not taken

| Alternative | For | Against |
|---|---|---|
| Leave translation off until the first `attach_device` | Nothing changes until a sandbox is used | The first attach would then switch every other device behind that unit from untranslated to translated at once: the same tables are needed anyway, and the switch would happen at an arbitrary moment instead of at boot |
| Pass through only the devices PCI enumeration finds | A device nobody enumerated cannot DMA | Requests also come from bridges' alias ids and devices firmware hides; blocking them is what Linux's "DMA aliases" handling exists for, and nothing here sandboxes yet |
| An identity-mapped domain where pass-through is missing | Works on units without ECAP.PT | A table mapping all of memory, kept current as memory is added; no unit seen so far lacks PT |

### As built

- `kernel/src/iommu_remap.rs`: `init`, `probe_unit`,
  `build_pass_through_tables`, `enable_unit`, `gcmd`, `sync_tables`, the
  invalidations, `write_context_entry`, `ContextEntry::{translated,
  pass_through}`, `remap_live_test`.
- `kernel/src/mm/page_table.rs`: `map_mmio`, the helper that maps device
  registers into the direct map.
- Tested under QEMU's emulated Intel IOMMU (`-device intel-iommu`), which
  the regular boot test does not attach.
