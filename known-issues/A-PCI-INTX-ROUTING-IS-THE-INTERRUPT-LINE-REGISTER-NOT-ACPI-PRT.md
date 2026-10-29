## A-PCI-INTX-ROUTING-IS-THE-INTERRUPT-LINE-REGISTER-NOT-ACPI-PRT (lane A, 2026-10-29) — OPEN

**Status:** OPEN

**In short:** When a PCI device uses its legacy interrupt wire (INTx), the
kernel must know which input of the interrupt controller that wire reaches.
It takes the answer from a byte the firmware writes into the device (the
Interrupt Line register). On the machines the boot test uses that byte is
right; on many real machines running in APIC mode it is not -- it names an
input of the old PIC, or is 0 -- and the true answer is in the ACPI table
`_PRT`, which the kernel does not read. A device whose wire is routed
somewhere else would then never deliver an interrupt.

**Where.** Every user of `PciDevice::irq_line` / the Interrupt Line register
as an I/O APIC input: the kernel's virtio-blk, virtio-net and rtl8139
drivers (`kernel/src/virtio/blk.rs`, `virtio/net.rs`, `rtl8139.rs`), the
boot-time INTx sweep (`pci::quiesce_unclaimed_intx`), and a userspace
driver's INTx through its device handle (`devhandle::intx_line`, design-
decisions 1579 as built 2026-10-29).

**Why it works today.** On QEMU's q35 with OVMF the firmware writes the line
the PIRQ router sends the pin to (10 or 11), and QEMU's ICH9 raises that
ISA line as well as the I/O APIC's GSI 16-23 for the pin, so the line read
from the register does fire. Every driver above relies on that.

**Who sees it.** Nothing yet: the test PC's devices that the kernel drives
use MSI or MSI-X (the e1000e, NVMe). It bites the first legacy-only device
on real hardware, or a machine whose firmware leaves the register unset.

**The proper fix.** Read the PCI routing table from ACPI -- `_PRT` under each
host bridge and PCI-to-PCI bridge, evaluated through the AML interpreter
(swizzling INTA-INTD across bridges by device number where a bridge has no
`_PRT` of its own) -- and give each function its GSI, polarity and trigger
mode from it, with the Interrupt Line register only as the fallback for a
machine with no ACPI routing. `PciDevice::irq_line` then carries the GSI,
and every user above follows.
