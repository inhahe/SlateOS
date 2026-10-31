## 1597. The kernel takes ACPI mode, and a power-button press shuts down in order

**Date:** 2026-10-31 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** Pressing a PC's power button while SlateOS runs now asks
SlateOS to shut down: files are written out and the machine powers itself
off. Running programs are not yet asked to save and stop (known-issues
A-POWER-BUTTON-SHUTDOWN-ASKS-NO-PROGRAM-TO-STOP). Before this, nothing in
SlateOS saw the press, and on the test PC the firmware cut the power at
once. To get the press, the kernel takes over ACPI mode from the firmware,
as Linux and Windows do. The power-button setting's default is "sleep",
which SlateOS cannot do yet, so a press shuts down instead and says so;
what the default should be is open-questions A-Q32.

**What was done** (`kernel/src/acpi/events.rs`; known-issues
A-THE-POWER-BUTTON-NEVER-REACHES-THE-KERNEL):

1. **The kernel takes the power button**, and ACPI mode with it where the
   firmware kept legacy mode, unless the platform is hardware-reduced, the
   FADT names no PM1 blocks, the button is a control-method device in
   legacy mode (below), or the SCI cannot be routed. At boot the kernel
   logs whether the firmware handed the chipset over in ACPI mode (PM1
   control's `SCI_EN`, or no SMI command port at all). Everything that can
   be checked is checked first: the SCI's pin (one the I/O APIC has, whose
   remapping entry was written), whose sense `ioapic::init` set, and the
   work queue. Only then, if the firmware kept legacy mode, does the kernel
   write the FADT's `ACPI_ENABLE` to its SMI command port and wait up to
   3 s for `SCI_EN`, as ACPICA does; a FADT whose `ACPI_ENABLE` and
   `ACPI_DISABLE` are both 0 offers no switch, and nothing is written.
   Should the SCI fail to unmask even so, ACPI mode taken is given back
   (`ACPI_DISABLE`) with every enable register the kernel rewrote put back
   as the firmware had it, so the firmware has the button again; where
   ACPI mode stays the OS's, every event is turned off instead, since
   nothing answers the SCI. In legacy mode the firmware acts on the button
   itself, which on the test PC means an immediate power cut. A button the
   FADT makes a control-method device (flag `PWR_BUTTON`), which needs AML,
   is left to the firmware where the firmware kept legacy mode; where ACPI
   mode is the OS's already the firmware will not act on it, so its fixed
   event is armed all the same. The PM1 and GPE blocks are read from the
   FADT's 64-bit fields where those are set in I/O space, else from the
   legacy ones -- which ACPI says to ignore only for a 64-bit value the OS
   can use, so they stand for one in memory (known-issues
   A-FADT-BLOCKS-OUTSIDE-IO-SPACE-ARE-NOT-REACHED).
2. **Only the power button's event is enabled.** Every general-purpose
   event (GPE) is disabled and its status cleared, and so is every fixed
   event's status. SlateOS runs no AML, so it could answer no GPE, and a
   level-triggered SCI left asserted by one would never stop. Of PM1
   enable's bits only the five that let a fixed event raise the SCI are
   written; `PCIEXP_WAKE_DIS` and the reserved bits stay as they were.
3. **The SCI** (ISA IRQ from the FADT, through any MADT override; each
   field the override leaves conforming is ACPI's SCI default,
   level-triggered and active-low; a PCI function sharing its line does
   not change that) is routed to a handler that clears the
   button's status before the EOI and hands the press to the kernel work
   queue. It also turns off any other event, fixed or general-purpose, it
   finds enabled and firing, one the firmware enabled again, whose level
   would otherwise hold the SCI asserted until the storm guard masked the
   pin, the button's with it.
4. **What a press does** is the power policy's
   (`fs::power::handle_power_button`): shut down runs
   `initproc::shutdown(PowerButton)` -- the kernel's service table marked
   stopped, filesystems synced, logs flushed; no program is asked to stop
   -- then `power::shutdown()`, the S5 power-off. A press while a shutdown
   is already under way adds nothing to it; one after a shutdown that
   halted without powering off powers off. Sleep and hibernation, which
   SlateOS cannot do, shut down instead, saying so. The screen off,
   log-out and lock screen, which it cannot do either and which are no way
   of stopping, do nothing, saying so.

**Alternatives:**

- **Leave legacy mode, and the button to the firmware.** No code, but a
  press is a power cut: nothing saved, no filesystem synced. That is
  harmless only while SlateOS may write nothing but a spare disk.
- **A press does nothing while the action configured is unavailable**
  (the default, sleep). That respects the setting literally, but leaves
  the button dead on every SlateOS system. The spec makes shutdown or sleep
  the default, so shutdown is within it.
- **Hand the press to userspace init**, which would ask its services and
  programs to stop. That is the end state, but init (lane D's) has no
  shutdown handling yet. What the kernel can do alone now is sync and power
  off, which already beats a power cut; the rest is known-issues
  A-POWER-BUTTON-SHUTDOWN-ASKS-NO-PROGRAM-TO-STOP.

**Consequences:** An AML interpreter's absence remains: lid, AC, battery,
thermal and embedded-controller events (all GPEs) are off. On a desktop
that costs nothing visible; on a laptop the lid and battery are not
reported. Every boot test presses the button through QEMU's
`system_powerdown` (`scripts/boot-test.sh`, `pwrbtn_responder`), with
`acpi.pwrbtn=hmp` making every press the harness's, recorded and never
acted on. It requires the press to reach the kernel's worker. First it
gives ACPI mode back and takes it again, which is the handoff a PC in
legacy mode needs, and enables the PM timer's fixed event behind the
kernel's back, which the SCI's handler must turn off when it fires.
