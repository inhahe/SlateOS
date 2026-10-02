## `TD-A-IRQ-REGISTER-CANNOT-NAME-THE-PCI-FUNCTION` — open, found 2026-08-22 (lane A)

**In short:** A userspace driver asks the kernel "wake me when interrupt line N
fires" by passing just the *number* of the line. But several devices can share
one line, and the kernel has a boot-time step that switches off the interrupt
pin on any device no driver has claimed. If that step already switched off the
device this driver cares about, the request still succeeds — and then the driver
waits forever for an interrupt that can never arrive. It now prints a warning
saying so, which is all it can honestly do: because the request names only a
line and not a device, the kernel cannot tell which of the devices on that line
the caller actually meant, so it cannot safely switch the right one back on.

### Where

- `kernel/src/syscall/handlers.rs` — `sys_irq_register`, which takes only
  `irq` in `arg0`.
- `kernel/src/pci.rs` — `quiesce_unclaimed_intx` (the sweep),
  `claim_intx` (the in-kernel path that does *not* have this problem), and
  `report_intx_silenced_on_irq` (the warning added alongside this entry).

### Why the in-kernel path is fine and this one is not

`claim_intx(addr)` takes a `PciAddress`. A driver initialising after the sweep
therefore re-enables *its own* function and nothing else, which is why the sweep
is documented as safe to run early. `sys_irq_register(irq)` has no equivalent:

| | identifies | can re-enable safely? |
|---|---|---|
| `claim_intx` | one function (bus:dev.fn) | yes — exactly that function |
| `sys_irq_register` | one IRQ line | **no** — the line may map to several functions, and the caller owns at most one |

Re-enabling every silenced function on the line would re-arm pins the caller has
no claim to, including whatever unclaimed device the sweep was quieting — which
is the storm the sweep exists to prevent. So the syscall warns and stops there.

### What the proper fix looks like

Give the interface the missing information, rather than making the kernel guess:

1. A `DeviceIrq` capability that names a **PCI function**, not a bare IRQ
   number, so the kernel can re-enable precisely the pin the holder owns. This
   is the shape the capability system already uses elsewhere — the
   `resource_id` is currently the IRQ number, which is what forces the guess.
2. Or a separate syscall taking a `PciAddress` that a userspace driver calls to
   claim its function's INTx, i.e. the userspace counterpart of `claim_intx`.

(1) is the better fit: it keeps authority and identity in the same token instead
of splitting them across two calls.

### Impact today

Low, and diagnosable rather than silent. No userspace PCI driver currently
depends on INTx delivery — the in-tree virtio drivers are either interrupt-driven
from inside the kernel (blk, net) or poll (gpu, sound). The warning is there so
that when one does, the failure reads as a named cause in the log instead of a
driver that simply never wakes up.
