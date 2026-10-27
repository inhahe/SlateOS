## TD-A-IOMMU-A-SANDBOXED-DEVICE-LOSES-ITS-RMRR-MEMORY (lane A, 2026-10-27) — OPEN

**Status:** OPEN.

**In short:** a PC's firmware lists memory some devices must always be able
to reach on their own (RMRRs: the integrated graphics' stolen memory, USB
controllers' legacy-keyboard buffers). Under DMA remapping every device
passes through untranslated (design-decisions §1576), so those devices are
fine. A device a driver asks to have sandboxed (`iommu_remap::attach_device`)
is held to the memory its driver maps, and loses its RMRR ranges: the
graphics or USB controller it is would stop working. Nothing sandboxes a
device yet, so nothing breaks today.

**Where:**
- `kernel/src/iommu.rs` parses only DMAR's DRHD structures (units), not its
  RMRR ones.
- `kernel/src/iommu_remap.rs`: `attach_device`.

**The proper fix:** parse each RMRR's range and device scope in
`iommu::init`. In `attach_device`, identity-map the device's RMRR ranges
into the domain before the context entry changes (what Linux does), or
refuse to sandbox such a device with an error that says why (what Linux
does for graphics and USB by default).

**Also untested on every boot:** the regular boot test attaches no IOMMU,
so the remapping code runs only on real hardware and in a boot with
`-device intel-iommu`. Attaching one in `scripts/boot-test.sh` would keep it
tested; what it costs a boot's time is not measured yet.
