## TD-A-XHCI-ENABLES-INTERRUPTS-IT-NEVER-HANDLES (lane A, 2026-10-27) — OPEN

**Status:** OPEN.

**In short:** the USB controller driver works by polling, but it switches
the controller's interrupts on (USBCMD.INTE, interrupter 0's IMAN.IE) and
leaves its legacy interrupt pin enabled, with nothing to handle them. Every
port change and transfer then asserts the pin; today the pin's line is
masked, so nothing comes of it. The day anything unmasks that line -- on the
test PC it is IRQ 11, shared by most of the chipset -- the line would be
asserted forever with nobody clearing it: an interrupt storm.

**Where:** `kernel/src/xhci.rs`, `XhciController::init` ("Enable Interrupter
0", "Enable interrupts and run"); `pci::quiesce_unclaimed_intx` skips the
controller because the driver claims it.

**The proper fix:** either leave the controller's interrupts off and its
INTx disabled while the driver polls, or give it an interrupt handler (MSI,
then the event ring drained in the handler) and stop polling. The second is
the better driver; the first is right until then.
