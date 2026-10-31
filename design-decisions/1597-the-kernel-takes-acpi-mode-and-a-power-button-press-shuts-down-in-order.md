## 1597. The kernel takes ACPI mode, and a power-button press shuts down in order

**Date:** 2026-10-31 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** Pressing a PC's power button while SlateOS runs now asks
SlateOS to shut down: services are stopped, files written out, then the
machine powers itself off. Before this, nothing in SlateOS saw the press,
and on the test PC the firmware cut the power at once. To get the press,
the kernel takes over ACPI mode from the firmware, as Linux and Windows
do. The power-button setting's default is "sleep", which SlateOS cannot do
yet, so a press shuts down instead and says so.

**What was done** (`kernel/src/acpi/events.rs`; known-issues
A-THE-POWER-BUTTON-NEVER-REACHES-THE-KERNEL):

1. **ACPI mode is the kernel's.** At boot the kernel logs whether the
   firmware handed the chipset over in ACPI mode (PM1 control's `SCI_EN`).
   If it did not, the kernel writes the FADT's `ACPI_ENABLE` to its SMI
   command port and waits up to 3 s for `SCI_EN`. In legacy mode the
   firmware acts on the button itself, which on the test PC means an
   immediate power cut.
2. **Only the power button's event is enabled.** Every general-purpose
   event (GPE) is disabled and its status cleared, and so is every fixed
   event's status. SlateOS runs no AML, so it could answer no GPE, and a
   level-triggered SCI left asserted by one would never stop.
3. **The SCI** (ISA IRQ from the FADT, through any MADT override, else
   level-triggered and active-low, as ACPI defines it) is routed to a
   handler that clears the button's status before the EOI and hands the
   press to the kernel work queue.
4. **What a press does** is the power policy's
   (`fs::power::handle_power_button`): shut down runs
   `initproc::shutdown(PowerButton)` -- services stopped, filesystems
   synced, logs flushed -- then `power::shutdown()`, the S5 power-off.
   Sleep and hibernation, which SlateOS cannot do, shut down instead,
   saying so. The screen off, log-out and lock screen, which it cannot do
   either and which are no way of stopping, do nothing, saying so.

**Alternatives:**

- **Leave legacy mode, and the button to the firmware.** No code, but a
  press is a power cut: nothing saved, no filesystem synced. That is
  harmless only while SlateOS may write nothing but a spare disk.
- **A press does nothing while the action configured is unavailable**
  (the default, sleep). That respects the setting literally, but leaves
  the button dead on every SlateOS system. The spec makes shutdown or sleep
  the default, so shutdown is within it.
- **Hand the press to userspace init**, which would stop its services
  itself. That is the end state, but init (lane D's) has no shutdown
  handling yet. The kernel's own orderly shutdown is what exists now, and a
  userspace power manager can take the event over when there is one.

**Consequences:** An AML interpreter's absence remains: lid, AC, battery,
thermal and embedded-controller events (all GPEs) are off. On a desktop
that costs nothing visible; on a laptop the lid and battery are not
reported. Every boot test presses the button through QEMU's
`system_powerdown` (`scripts/boot-test.sh`, `pwrbtn_responder`) and
requires the press to reach the kernel's worker.
