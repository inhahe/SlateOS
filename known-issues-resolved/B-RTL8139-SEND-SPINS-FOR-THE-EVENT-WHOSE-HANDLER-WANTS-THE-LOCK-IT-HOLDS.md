### [A] B-RTL8139-SEND-SPINS-FOR-THE-EVENT-WHOSE-HANDLER-WANTS-THE-LOCK-IT-HOLDS. `send()` polls 100 000 times for TX-complete while holding the lock `handle_irq` blocks on — 2026-08-14 — **FIXED** (`64f7d2fd9`)

> **Resolution.** As with the console entry above, the fix landed in the same
> commit that added the write-up, so the "**Proper fix.**" paragraph below
> describes work already done. Re-verified 2026-08-14: all four `DEVICE`
> acquisitions in `kernel/src/rtl8139.rs` are `lock_irqsave()` (lines 364,
> 375, 573, 605) and none is a plain `lock()`. The *deadlock* is closed; the
> 100 000-iteration busy-wait inside `send` is deliberately **still there**
> and is still worth fixing — see the final paragraph of this entry for the
> block-and-wake rewrite, which remains open work.

**Found by auditing the bug *class* rather than the bug.** After fixing
B-CONSOLE-LOCK-IS-TAKEN-FROM-A-HARD-IRQ above, the obvious question was
whether the console was the only place a hard-IRQ handler blocks on a
lock that task context also takes. `handle_device_irq`
(`kernel/src/ioapic.rs:731`) is the sole hard-IRQ device dispatch, so the
audit is bounded and can be made *complete* rather than sampled:

| Callee in `handle_device_irq` | Verdict |
|---|---|
| `cputime::enter_irq` / `exit_irq` | lock-free (atomics) |
| `ktrace::record` | lock-free |
| `keyboard::handle_scancode` (irq 1) | was the console bug — now fixed |
| `mouse::handle_irq` (irq 12) | lock-free |
| `virtio::blk::handle_irq` | lock-free (atomics + port I/O) |
| `virtio::net::handle_irq` | lock-free (its `DEVICE` is never touched from IRQ) |
| `rtl8139::handle_irq` | **plain `DEVICE.lock()` — this entry** |
| `irq_notify`, `irq_storm::record_irq` | lock-free |
| `sched::try_wake` | `SCHED.try_lock()` — correct by design, returns false and raises a softirq |
| `apic::eoi` | lock-free |
| `softirq::process_pending` | re-enables interrupts first — different class, see below |

One finding. `e1000` was checked too and is clean *for a different reason*:
it has no `handle_irq` at all (it is polled), so its `DEVICE` is
task-context-only.

**The bug.** `rtl8139::handle_irq` (`kernel/src/rtl8139.rs:553`) does
`let guard = DEVICE.lock();` in hard-IRQ context. The same `DEVICE`
(line 181) is taken in task context by `with_device` (line 361), which is
what `send` (366) and `recv` (372) go through — and `with_device` holds
the lock across the whole closure.

**Why this one is worse than the console.** Look at what `send` does while
holding the lock (`kernel/src/rtl8139.rs:392`):

```rust
// Wait for the descriptor to become available (OWN bit clear
// means hardware finished with it).
for _ in 0..100_000u32 {
    let status = unsafe { port::inl(self.io_base + status_reg) };
    if status & TX_STATUS_OWN == 0 { break; }
}
```

The OWN bit is cleared by the hardware finishing the previous transmit —
**which is precisely the event that raises the TX-complete interrupt.** So
the code spins, holding `DEVICE`, waiting for the exact hardware event
whose interrupt handler will block on `DEVICE`. This is not a narrow race
window that a busy system might hit; it is a loop that waits for the
trigger while holding the trigger handler's lock. On any TX-active link
the interrupt lands inside that loop essentially by construction.

**Why it hasn't been seen.** The RTL8139 is not the NIC the QEMU boot test
runs — virtio-net and e1000 are — so `handle_irq` never fires here. The
driver is untested-in-anger, not correct.

**One mitigating difference from the console bug:** `DEVICE` is already a
`crate::sync::Mutex` (`use crate::sync::Mutex` at line 26), so the 30-second
stall detector *will* fire and name the lock. This hangs loudly rather than
silently. It still hangs.

**Proper fix.** Change all `DEVICE` acquisitions in `rtl8139.rs` to
`lock_irqsave()` — the same structural fix as the console, for the same
reason. Note that on this driver `lock_irqsave` inside `send` is not merely
protective: it is what makes the poll loop terminate, because with the
interrupt masked the handler cannot run at all until `send` releases, and
the OWN bit is observable by polling regardless of whether the interrupt
was delivered.

Separately, the 100 000-iteration poll while holding a lock is bad shape on
its own merits (it is a busy-wait for a device with no bound in time). The
right long-term structure is the one `virtio::blk` already uses: the ISR
acknowledges at the device with atomics only and wakes a task, and the
descriptor wait becomes a block-and-wake rather than a spin. That is a
driver rewrite, so it is not folded into the deadlock fix.

**The exception class was audited too, and is clean — by design, not by
luck.** This matters because `cli` does not mask faults, so an exception
handler that blocks on a task-held lock cannot be fixed by `lock_irqsave`
at all; it has to use `try_lock`. Checked:

* `idt.rs` itself takes **no** locks (0 acquisition sites in the file).
* `mm::fault::resolve` (`kernel/src/mm/fault.rs:262`) uses
  `KERNEL_AS.try_lock().ok_or(KernelError::PageFault)?`, with a comment
  naming the exact hazard: *"if we faulted while holding this lock (e.g.,
  during VMA manipulation), the fault is in critical code and cannot be
  resolved."* `add_kernel_vma`/`remove_kernel_vma` (290, 299) keep the plain
  `lock()`, correctly — they are task-context-only and the fault path never
  blocks on them.
* `proc::pcb::try_resolve_fault` (`kernel/src/proc/pcb.rs:5370`) uses
  `PROCESS_TABLE.try_lock()`, and further drops the guard *before* CoW
  resolution because that path allocates.

**The pattern worth noticing.** The memory-management code was written with
this hazard in mind throughout — `try_lock` plus a comment explaining the
re-entrancy every time. The device and console code was not: plain `lock()`
everywhere, no comment, no defence. The discipline exists in one half of the
tree and is absent from the other, which is why both bugs found so far are
in drivers/console and none are in `mm`. When auditing further, weight
driver code accordingly.

**Audit scope note, so the next session knows what was *not* covered.**
Softirq handlers (`softirq::process_pending`, called after EOI with
interrupts *re-enabled*) are a third class: they can be interrupted by a
further device IRQ, so a lock shared between a softirq handler and a
hard-IRQ handler has the same failure mode. That intersection is currently
empty because the only hard-IRQ lock acquisition in the whole tree is the
`rtl8139` one above — but it stops being empty the moment another ISR
learns to take a lock, so the check has to be redone whenever one does.
