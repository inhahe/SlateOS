## 1585. A Linux driver process's interrupts are its device's MSI, taken at each turn of its loop and waited for when it is idle

**Date:** 2026-10-29 · **Decided by:** Claude (operator-approved scope) -- the operator chose Linux's drivers through a layer (§1577); how the layer delivers interrupts is Claude's call, under §1578 and §1579 · **Lane:** A

**In short:** When a device needs attention -- a packet arrived, a
transfer finished -- it raises an interrupt, and the driver's interrupt
handler runs. In a driver process the kernel receives the interrupt and
counts it on the device's handle (§1579). The layer now hands those
interrupts to the handler a Linux driver registered, the way Linux would
run it, and while the process has nothing else to do it waits for its
device as well as for its timers.

### The decision

1. **Linux's MSI API, one vector per function** (`linuxdrv/emul/lx_irq.c`):
   `pci_alloc_irq_vectors()` and `pci_enable_msi()` set up the function's
   MSI through the handle -- the kernel programs it, once for the handle's
   life -- and give it an interrupt number of its own; `pci_irq_vector()`
   names it. MSI-X is refused, which drivers take as the cue to fall back to
   MSI (e1000e does), and the legacy line is not routed: the handle offers
   one MSI vector, and nothing else.
2. **The layer's own interrupt core, not Linux's**: `request_threaded_irq()`,
   `free_irq()`, `disable_irq()`/`enable_irq()` and `synchronize_irq()` over
   that one vector. Linux's `kernel/irq` and `drivers/pci/msi` program
   interrupt controllers and hang every interrupt on irq domains, which the
   kernel does here; compiling them would mean faking a controller for them
   to program.
3. **Taken at each turn of the loop** (`lx_irq_poll`, before the turn's
   softirqs, as Linux's `irq_exit()` runs them): each function with a
   handler requested and enabled has its interrupts taken, without waiting,
   and its handler run once for however many came -- an MSI is an edge, and
   Linux's handlers read their device's cause register -- in hardirq context
   with interrupts off.
4. **A threaded handler's second half runs in a kthread of its own**, as
   Linux's `irq/N-name` thread runs it, woken when the first half answers
   `IRQ_WAKE_THREAD`; a ONESHOT interrupt is not taken again until it has
   finished, and `synchronize_irq()` waits for it.
5. **Held, not lost**: while no handler is requested, or the interrupt is
   disabled, its interrupts stay counted in the kernel, not taken, and are
   delivered once it is enabled, as Linux replays a pending interrupt.
6. **The idle wait includes the device** (`linuxdrv/kit`): with nothing to
   run, the loop waits on every function with a live handler and the next
   timer together (`SYS_WAIT_MULTIPLE`), for ever when no timer is due. A
   process with a live handler therefore never ends by itself: a driver
   lives as long as its device can call on it.

### The alternatives

- **Linux's own interrupt core and MSI code over a fake interrupt
  controller**: Linux's semantics everywhere, but an irq domain, an irq
  chip and an MSI domain to write and keep in step, for one vector the
  kernel has already programmed.
- **A thread of the process blocked in `SYS_PCI_DEVICE_IRQ_WAIT`**: closer
  to a real interrupt's latency, but a second thread breaks the layer's
  one-thread model (§1578 rule 5), under which Linux's own locking is
  correct as compiled.
- **Polling the device only when idle**: one system call fewer per turn,
  but an interrupt that comes while tasks run would wait until all of them
  block.

### What it costs

- One system call per turn for each function with a live handler, to take
  its interrupts.
- A handler runs between tasks, never in the middle of one: latency is the
  longest a task runs without blocking or calling `cond_resched()`.
- One MSI vector per function: a driver that wants a vector per queue
  (MSI-X) runs with one.

### Where it lands

`linuxdrv/emul/lx_irq.c` (the MSI API, the interrupt core, the turn's
poll), `linuxdrv/emul/lx_softirq.c` (the poll in the turn),
`linuxdrv/kit/src/pci.rs` (`slx_pci_irq_setup`, `_take`, `_listen`, the
idle wait), `linuxdrv/kit/src/sched.rs` (waiting on the devices when
idle), and hello's "pci" run (a handler, a disabled interrupt held and
delivered, a threaded handler).
