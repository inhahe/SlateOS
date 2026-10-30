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
   or 0 for a function with no pin; turning MSI on changes `dev->irq`, and
   turning it off changes it back, as on Linux. So a legacy driver's
   `request_irq(pdev->irq)` reaches its own function even when two share a
   line, and `pci_alloc_irq_vectors()` with `PCI_IRQ_INTX` and
   `pci_irq_vector()` answer it, as Linux's do. The line is set up through
   the device handle (`slx_pci_intx_setup`, the kit's) when a handler is
   first asked for on it; turning MSI or MSI-X on replaces it.
2. **Handled, then taken.** The kernel quenches the function at each
   interrupt (its INTx-disable bit) and re-arms it when the interrupt is
   taken (§1579). Each turn of the loop looks without taking
   (`slx_pci_irq_ready`: a `SYS_WAIT_MULTIPLE` that waits for nothing),
   runs the handler in hardirq context, and only then takes -- Linux's
   order for a level interrupt: mask, handle, unmask.
3. **Masked while Linux would keep it masked.** While the handler is
   disabled the line is left quenched, its interrupt owed and run once
   enabled; a oneshot threaded handler's line is re-armed only once its
   thread has run, and the loop does not wait on a line so held (it would
   end every wait at once). A threaded handler with no first half and no
   `IRQF_ONESHOT` is refused, as Linux refuses it on a level line. An
   interrupt with no handler requested is taken and dropped.

### Alternatives

- **Taken first, as MSI is** -- one path for every kind. The line would be
  re-armed with the device still asserting, interrupt again before the
  handler had read its cause, and run the handler once more for nothing:
  an extra hardware interrupt and a spurious handler call for every real
  one. A message is an edge and has no such cost.
- **A kernel call to re-arm separately** (take without re-arming, then
  re-arm): the same order, but a change to the device handle's ABI, where
  looking without taking already gives it.
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
re-armed after it.
