## 1591. A Linux driver process takes a legacy interrupt only after its handler has run

**Date:** 2026-10-30 · **Decided by:** Claude (operator-approved scope) -- INTx for Linux's drivers is in roadmap 2.1's core shim, over the device handle's INTx (§1579 as built); how the layer delivers it is Claude's call, under §1578 · **Lane:** A

**In short:** Old-style PCI interrupts ("INTx", one wire per device that
stays raised until the driver deals with the device) now reach Linux's
drivers in a driver process, as message interrupts (MSI) already did. The
kernel silences such a device each time it interrupts and lets it
interrupt again when the driver process collects the interrupt. The layer
collects it only after the driver's handler has run -- as Linux itself
does -- so a device the handler has just quietened does not interrupt a
second time for nothing.

### The decision

1. **A function's legacy interrupt is a number of the layer's own**
   (`linuxdrv/emul/lx_irq.c`): `pcibios_alloc_irq()` -- the hook x86 uses
   to route a device's line into `dev->irq` before its driver probes --
   puts there a number for the function alone, above every function's
   message vectors and every value the PCI Interrupt Line register holds,
   or 0 for a function with no pin, or none the firmware routed (an
   Interrupt Line of 0 or 0xff); turning MSI on changes `dev->irq`, and
   turning it off changes it back, as on Linux. The line keeps a record of
   its own, apart from the message vectors', so a number kept from MSI is
   never the line's. So a legacy driver's `request_irq(pdev->irq)` reaches
   its own function even when two share a line, and
   `pci_alloc_irq_vectors()` with `PCI_IRQ_INTX` and `pci_irq_vector()`
   answer it, as Linux's do. The line is set up through the device handle
   (`slx_pci_intx_setup`, the kit's) when a handler is first asked for on
   it while neither MSI nor MSI-X is on, after the request is checked, so
   a refused request changes nothing. Turning MSI or MSI-X on replaces the
   line's set-up in the kernel; its handler stays the line's, never run
   while they are on, and is delivered again once they are off.
2. **Handled, then taken.** The kernel quenches the function at each
   interrupt (its INTx-disable bit) and re-arms it when the interrupt is
   taken (§1579). Each turn of the loop looks without taking
   (`slx_pci_irq_ready`: `SYS_PCI_DEVICE_IRQ_PENDING` with
   `IRQ_PENDING_LOOK`, which answers whether any are counted, taking and
   re-arming nothing), runs the handler in hardirq context, and only then
   takes -- Linux's order for a level interrupt: mask, handle, unmask.
3. **Masked while Linux would keep it masked.** While the handler is
   disabled, or none is requested, the line is left quenched, its
   interrupt owed and run once a handler is enabled or requested -- Linux
   keeps a level line with no action masked. A oneshot threaded handler's
   line is re-armed only once its thread has run and its handler is
   enabled, and the loop does not wait on a line so held (it would end
   every wait at once). A threaded handler with no first half and no
   `IRQF_ONESHOT` is refused, as Linux refuses it on a level line.

### Alternatives

- **Taken first, as MSI is** -- one path for every kind. The line would be
  re-armed with the device still asserting, interrupt again before the
  handler had read its cause, and run the handler once more for nothing:
  an extra hardware interrupt and a spurious handler call for every real
  one. A message is an edge and has no such cost.
- **A kernel call to re-arm separately** (take without re-arming, then
  re-arm): the same order, but two calls each turn and a second meaning
  for a take, where a look -- a mode of the call that takes -- gives it
  with take unchanged.
- **Looking through `SYS_WAIT_MULTIPLE`**, a wait for nothing on the
  handle: no new mode, but every turn of an INTx driver's loop paid the
  wait's allocations and a waiter's registration to read one counter.
  The first version; the look replaced it.
- **The raw Interrupt Line value as `dev->irq`**, as Linux's PCI core
  leaves it: two functions on one line would share a number, and a
  request would not say which function it meant.

### Checked by

`linuxdrv/hello`'s PCI run (`hello_pci_intx`), on the boot test's spare
82574 before its MSI: the number the layer's own; a handler run in hardirq
context exactly once per interrupt -- a take before the handler would run
it twice -- and the line re-armed for the next; an interrupt held while
disabled, delivered once enabled; the refusal of a threaded handler with
no first half and no `ONESHOT`; a oneshot thread run in its task, the line
re-armed after it. Then, in its MSI run (`hello_pci_interrupts`): an
interrupt the kernel counts seen by two looks, taking nothing; the line
delivering when set up again after MSI; a number kept from MSI refused on
the line; MSI turned on over the line's handler -- asked for again while
MSI is on -- the handler not run and the MSI dropped, then the line
delivering once MSI is off. `devhandle`'s INTx self-test checks the
kernel's look takes and re-arms nothing.
