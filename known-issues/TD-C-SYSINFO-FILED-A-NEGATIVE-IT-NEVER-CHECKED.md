## TD-C-SYSINFO-FILED-A-NEGATIVE-IT-NEVER-CHECKED -- PARTLY FIXED 2026-09-15

**In short:** the System Information window has nine categories -- PCI, USB,
sound, I/O ports, DMA, the firmware memory map, drivers, services and startup
items -- that say "cannot read". Yesterday I filed an entry saying that was
correct, because "nothing publishes these". I had only looked in one place.
Three of the nine have a source in `/proc` today that fills nearly every field
the window shows, and several others have a partial one.

**Date:** 2026-09-15. **Lane:** C. **Decided by:** Claude (autonomous) -- the
error is mine and this entry is the correction.

**Status 2026-09-15, later the same day.** Lane B built the three parsers, and
I/O Ports, Drivers and Startup Items now read `/proc/ioport`, `/proc/kmod` and
`/proc/autostart`. `DriverInfo::path` is left empty and `StartupEntry::source`
was renamed to `phase` rather than filled with one. **Still open:** PCI, USB
and DMA stay unwired for the reasons below -- one field of eight is not a
category -- and the memory map and Services wait on lane A, who has been asked
for per-row output from `memlayout` and `servicemgr`.

**How the wrong claim was made.** The nine categories read
`/sys/hardware/{pci,usb,sound,ioports,memmap,dma}`, `/sys/services`,
`/sys/drivers`, `/sys/startup`. I checked the kernel's **sysfs**, found no such
nodes, and noted that §850 puts hardware under `/sys/devices` anyway, so no
producer will ever appear at those paths. All true. Then I wrote down
"nothing publishes these" -- a claim about the *whole kernel* -- on the
strength of having looked at one filesystem. `kernel/src/fs/procfs.rs` has
~470 generators and I had read none of them.

This is the same defect this lane has spent the week finding in other people's
code, in its own words: **a count without the thing that would let someone
check it is read as coverage.** The table row carried no column saying where I
had looked, so nobody reading it -- including me, the next day -- could tell
that it meant "not in sysfs" rather than "not anywhere".

**And then I overcorrected.** The first version of this entry said eight of
the nine had sources "listing exactly the rows the window wants". That was the
identical mistake pointed the other way: I had matched *categories* to
*generators* by name and not checked a single field. Corrected below by
reading both sides. The verdict is much less tidy than either claim.

**What is actually there, field by field.**

| category | struct fields | source | fields with a source |
|---|---|---|---|
| I/O ports | `start, end, device` | `/proc/ioport` per-region | **3 of 3** |
| drivers | `name, path, status` | `/proc/kmod` per-module | 2 of 3 (no path; has version, size, refcount the struct lacks) |
| startup | `name, path, source` | `/proc/autostart` per-item | 2-3 of 3 (`path` ← command, `source` ← phase) |
| sound | `name, device_type, driver, status` | `/proc/audiodevice` per-device | 2 of 4 (`device_type` ← direction; no driver, no status) |
| PCI | `bus, device, function, vendor_id, device_id, class, description, vendor_name` | `/proc/devicemgr` per-device | **1 of 8** -- `description` only |
| USB | `port, vendor_id, product_id, description, speed` | `/proc/devicemgr` per-device | **1 of 5** -- `description` only |
| DMA | `channel, device, mode` | `/proc/dmastat` | **1 of 3, and mismatched** |
| memory map | `start, end, region_type, description` | none | **0** |
| services | `name, status, start_type` | none | **0** |

**Three of those rows need their mismatch stated, because a name match is not
a data match:**

- **`/proc/dmastat` is not DMA channels.** It reports IOMMU mapping statistics
  per device -- maps, active mappings, bytes, faults. The category means the
  legacy 8237 channels 0-7, which have a channel number and a transfer mode.
  There is no channel number in the file because the subsystem does not model
  one.
- **`/proc/iomem` is not the firmware memory map.** It lists MMIO regions *the
  kernel has mapped* -- `iomem::register(name, base, size)`, called for the
  LAPIC, IOAPIC, HPET, a device BAR. The category means the map the firmware
  hands over at boot: usable RAM, reserved, ACPI reclaim. Nothing in `kernel/`
  matches `e820` at all.
- **`/proc/devicemgr`'s `bus` is a bus *type*, not a bus *number*.** It is the
  string "PCI" or "USB", which is how you would tell the two categories apart
  -- and is not `PciDeviceInfo::bus`, which wants the 8-bit bus number of a
  `bus:device.function` address. Wiring one to the other would put `PCI` in a
  column headed Bus and it would look like data.

**Two gaps are the kernel publishing less than it holds, and those are worth
asking about rather than working around:**

- `memlayout` tracks regions -- `gen_memlayout` prints `region_count`,
  `total_ram`, `total_reserved`, `total_kernel` -- and publishes **no
  per-region row**. The rows exist; only the totals are served.
- `servicemgr` tracks services -- `gen_servicemgr` prints `service_count`,
  `running`, `total_starts`, `total_stops`, `total_failures` -- and publishes
  **no per-service row**. Same shape. `/proc/svcstart` is about the startup
  orchestrator's phase and backoff policy, not about which services are up.

**What the fix is.**

1. **Wire the three that fit** -- I/O ports, drivers, startup -- leaving the
   unpublished fields (`DriverInfo::path`) empty rather than filled, exactly as
   Display's adapter fields were left empty today.
2. **Parse in `procinfo`, not here.** Settled by the IRQ/monitors request and
   lane B's agreement: two `/proc` parsers in one repository is the arrangement
   where a kernel change fixes one program and not the other. `userspace/` will
   want `lsmod` eventually.
3. **Ask lane A for per-row output from `memlayout` and `servicemgr`.** Both
   hold the rows already.
4. **Leave PCI, USB and DMA saying they cannot be read**, because one field in
   eight is not a category, and a window with a Vendor column full of blanks
   next to a Description full of values invites the reader to assume the blanks
   are the machine's rather than ours.

**Until then the current state is safe:** the categories say they cannot be
read, which remains true of the paths they name. What is lost is only that
three of them could be showing real data.

**The lesson, stated so it outlives the entry:** a negative finding needs its
search recorded beside it. "Nothing publishes X" is a claim about everywhere,
and the honest form of what I knew was "no sysfs node publishes X; I have not
looked at procfs". Write the second one. It is the one that tells the next
reader what is left to do.
