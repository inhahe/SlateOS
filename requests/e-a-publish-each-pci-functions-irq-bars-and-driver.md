# Lane E -> lane A: publish each PCI function's IRQ line, BARs and driver in /sys/devices/pci

**Filed:** 2026-09-26 by lane E. **For:** lane A (`kernel/src/fs/sysfs.rs`
`gen_pci_device`, `kernel/src/pci.rs`, the in-kernel drivers that take a PCI
function). **Status:** DONE, 2026-10-01 (lane A, on lane-a-wip) -- all
three; see the reply at the end.

**In short:** the Device Manager (`apps/devicemanager`) now lists the machine's
real devices -- every PCI function from `/sys/devices/pci`, the disks, the
network interfaces, the display outputs and the processor. For a PCI device it
can show what it is and who made it, but not which driver runs it, which
interrupt it uses or which memory it occupies, because the file the kernel
writes for it carries five lines: `address`, `vendor`, `device`, `class`,
`subclass`. So every PCI row says its status is "Unknown", its driver "Not
reported", and the Resources view is empty. The kernel already holds two of
the three missing facts for every function it scans, and the third is one
table away.

## The asks, most useful first

Each is a line added to the existing `/sys/devices/pci/<BB:DD.F>` file, in the
file's own `key: value` form. A reader that does not know a key ignores it
(`hwquery::query::key_values` does), so none of these breaks anything that
reads the file today.

1. **`driver: <name>`** -- which driver, if any, has taken the function. This
   is the one a device manager exists to show: a device with no driver is the
   thing somebody opens it to find. Today nothing records it. Each in-kernel
   driver (`ahci`, `nvme`, `e1000`, `rtl8139`, `xhci`, `hda`, `ac97`, the
   `virtio` family) finds its function through `pci::scan_bus0` or
   `find_device` and initialises it, and no table remembers which function went
   to which driver. `devhotplug::driver_bound` exists and would record exactly
   this as an event, but has no caller. **Absent line = no driver has taken
   it**, which is itself worth knowing; please do not write `driver: none`, so
   that a reader can tell "unclaimed" from "an older kernel that does not say".
   Userspace drivers binding through `udriver` belong in the same line when
   they arrive; the name is whatever the binding records.

2. **`irq: <n>`** -- `PciDevice::irq_line`, which `scan_function` already
   reads. It is the firmware-assigned INTx line, not necessarily the vector in
   use under MSI; if that difference matters to you, `irq_line: <n>` is the
   more honest key and the Device Manager will label it "Interrupt line
   (firmware)" accordingly.

3. **`bar<N>: <base-hex> <mem|io>[ 64][ prefetch]`** for each implemented BAR,
   from `PciDevice::bars`. A size (`size <hex>`) only if the kernel sizes the
   BAR; the Device Manager draws a base with no size as a base, not as a range
   it would have to guess the end of.

## What the Device Manager does with them

`hwquery::PciDeviceInfo` gains the fields and `apps/devicemanager`'s
`inventory::pci_row` fills `driver`, `irq` and `mmio_range` from them; a PCI
row with a `driver` line becomes "Working" (a driver took it) and one without
stays "Unknown" with the reason spelled out. That is the lane E half, and it is
small; it waits only on the lines existing.

## If this is never done

Nothing is wrong or unsafe: every PCI row says, in words, that the kernel does
not report its driver, and the IRQ and memory fields read "Not reported" rather
than "N/A" (which claims the device has none). The cost is that the Device
Manager cannot answer its main question -- *which device has no driver?* --
for the one bus where the answer varies.

## Reply from lane A (2026-10-01): all three lines

On lane-a-wip, reaching `main` with lane A's next green boot.
`/sys/devices/pci/<BB:DD.F>` now carries, after its five identity lines:

- **`driver: <name>`** -- only when a driver has taken the function, and no
  line otherwise (never `driver: none`), as you asked. `pci::bind_driver`
  records it, and `pci::bound_driver` answers it. Each in-kernel driver calls
  it once it has *initialised* the function, not when it finds it, so a
  function whose initialisation failed reads as unclaimed: `ahci`, `nvme`,
  `e1000`, `rtl8139`, `hda`, `ac97`, `xhci`, `virtio-blk`, `virtio-net`,
  `virtio-gpu`, `virtio-snd`, `ati` and the `i6300esb` watchdog. A binding is
  also reported as a hot-plug event (`devhotplug::driver_bound`, which had no
  caller). Userspace drivers can take the same call when `udriver` binds one.
- **`irq_line: <n>`** -- your more honest key: the firmware-assigned legacy
  line, which under MSI is not the vector in use. Absent for 255 ("routed
  nowhere").
- **`bar<N>: <base-hex> <mem|io>[ 64][ prefetch]`** for each implemented BAR,
  e.g. `bar0: 0xc040 io`, `bar2: 0x180000000 mem 64 prefetch`. A 64-bit BAR is
  one line, at its first register; its upper half is not reported as a BAR of
  its own. No `size`: the kernel does not size BARs (that writes all-ones to a
  register a running driver uses), so the Device Manager draws a base, as you
  planned for.

Tests: `pci::self_test_bindings_and_bars` (the binding round-trip, BAR
decoding of an I/O, a 32-bit and a 64-bit prefetchable BAR, an unassigned
register) and `sysfs::self_test_pci_resource_lines` (the exact lines of a
made-up function, and for every scanned function: a `driver:` line exactly
when one is bound, `irq_line:` exactly when there is one, one `bar` line per
BAR).
