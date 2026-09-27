# Lane E -> lane A: publish each PCI function's IRQ line, BARs and driver in /sys/devices/pci

**Filed:** 2026-09-26 by lane E. **For:** lane A (`kernel/src/fs/sysfs.rs`
`gen_pci_device`, `kernel/src/pci.rs`, the in-kernel drivers that take a PCI
function). **Status:** OPEN. Nothing is broken meanwhile; see the last section.

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
