## A-FADT-BLOCKS-OUTSIDE-IO-SPACE-ARE-NOT-REACHED (lane A, 2026-10-31) — OPEN

**Status:** OPEN -- no PC SlateOS has run on is known to need it; the boot
log says so on one that does

**In short:** the FADT (the ACPI table naming the power-management
registers) can place a register block in memory rather than among the I/O
ports. SlateOS reaches these blocks only as I/O ports. Where the FADT also
gives the block's old 32-bit I/O-port field, SlateOS uses that, as ACPI
allows; where it gives the memory address alone, the block is missing to
SlateOS. A missing PM1 control block means a shutdown halts without
powering off; a missing PM1 event block means the power button stays the
firmware's; a missing GPE block means the firmware's general-purpose
events stay enabled.

**Where.** `kernel/src/acpi/fadt.rs`, `block_port` and `parse_fadt`'s
`x_block`: an `X_PM1a_EVT_BLK`, `X_PM1b_EVT_BLK`, `X_PM1a_CNT_BLK`,
`X_PM1b_CNT_BLK`, `X_GPE0_BLK` or `X_GPE1_BLK` whose Generic Address
Structure is not in I/O space (address space 1), or names a port past
0xFFFF. `PowerInfo` holds every block as a `u16` port, and
`kernel/src/acpi/events.rs` and `kernel/src/power.rs` reach them with
`port::inw`/`outw`/`inb`/`outb`.

**How to see it.** The boot log line `[acpi]   X_<name>: address space
<n>, address <a>, beyond SlateOS's reach (known-issues
A-FADT-BLOCKS-OUTSIDE-IO-SPACE-ARE-NOT-REACHED): no <name>` names the
block and where the FADT put it; `... the legacy <name>, <port>, used
instead` is the case that still works.

**The proper fix.** A register type for a Generic Address Structure --
an I/O port, or a physical address reached through the HHDM, as
`power.rs` already reaches a memory-space reset register -- with 8-, 16-
and 32-bit reads and writes, held in `PowerInfo` for each block in place
of the `u16`, and used by `events.rs` and `power.rs` for every access.
ACPICA does the same (`acpi_hw_read`/`acpi_hw_write` on a GAS).

Trigger: a boot log on real hardware with the `no <name>` line.
