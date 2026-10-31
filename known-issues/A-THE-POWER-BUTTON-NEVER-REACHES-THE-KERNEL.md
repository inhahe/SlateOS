## A-THE-POWER-BUTTON-NEVER-REACHES-THE-KERNEL (lane A, 2026-10-31) — OPEN

**Status:** OPEN

**In short:** pressing a PC's power button while SlateOS runs does not ask
SlateOS to shut down: no kernel code handles the press. On the test PC a
short press still turns the machine off (the operator saw it, 2026-10-31),
most likely the firmware doing it as a power cut: no program is told,
nothing is saved, no filesystem is synced. Under a SlateOS that could write
its disks, that is the "unexpected power loss" every filesystem journal
exists for.

**Not yet explained:** the operator saw new lines appear in SlateOS's
on-screen log well within half a second of the press, just before the
power went. A firmware power cut would print nothing first, so something
in SlateOS may notice the press. The first step of the fix settles it: log
at boot whether the firmware handed the chipset over in ACPI mode (`SCI_EN`
in PM1 control), and record the PiKVM's video at its full frame rate
across a press, to read those lines.

**Where.** The pieces exist but nothing joins them:
- `kernel/src/acpi/fadt.rs` parses the FADT -- `sci_interrupt`, `smi_cmd`,
  `acpi_enable`, the PM1 event and control blocks -- and powers off into S5.
- `kernel/src/fs/power.rs` holds the power button's policy
  (`handle_power_button`, `PowerAction`); only its own self-test calls it.
- `kernel/src/initproc.rs` has `ShutdownReason::PowerButton`.

Missing: the kernel never switches the chipset into ACPI mode (writing
`acpi_enable` to `SMI_CMD` when `SCI_EN` is clear, the likely reason the
firmware still acts on the button itself), never enables the power-button
event (`PWRBTN_EN` in PM1 enable), and has no handler on the SCI interrupt
to read and clear `PWRBTN_STS`.

**The proper fix.** Enter ACPI mode at boot. Route the FADT's SCI
interrupt through the I/O APIC (level-triggered, active-low, and through
interrupt remapping). Enable the power-button fixed event. In the handler,
clear `PWRBTN_STS` and hand the press to `power::handle_power_button`,
whose action for the button starts an orderly shutdown through init
(`ShutdownReason::PowerButton`) that ends in `fadt`'s S5 power-off. Check
it in QEMU, whose `system_powerdown` monitor command presses the ACPI power
button (`scripts/boot-test.sh` already drives the monitor), and on the
test PC through the PiKVM's short press (`scripts/pikvm.py power off`).

Trigger: the next lane A task; before any SlateOS run that writes a disk
holding something worth keeping.
