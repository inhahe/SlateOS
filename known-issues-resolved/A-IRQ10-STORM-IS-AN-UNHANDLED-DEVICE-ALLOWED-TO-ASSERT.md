## `A-IRQ10-STORM-IS-AN-UNHANDLED-DEVICE-ALLOWED-TO-ASSERT` — fixed 2026-08-22 (lane A)

**In short:** Several plug-in devices share a single "please look at me" wire to
the CPU. That wire works by being *held down* until whichever device pulled it
lets go — and only that device's own driver knows how to make it let go. Six of
the eight devices sharing this wire have drivers that never listen to it, so if
one of them pulled the wire, nothing ever released it and the CPU spent all its
time answering a call nobody would hang up: about half a million times a second,
enough to starve everything else and, once, to wedge a boot. The fix tells every
device whose interrupt nobody listens for that it is not allowed to pull the wire
at all.

The fix's own logging then showed the problem was **worse than the report said**:
three devices were holding a wire down with nobody listening, and only one of
them was on the wire the original report named. A *second* shared wire had the
same latent fault and had simply not been unlucky yet.

### Why an unhandled interrupt is not merely wasteful here

An unhandled *edge*-triggered interrupt costs one wasted trip through the
handler. An unhandled *level*-triggered one costs the machine, because the
condition that caused it is still true when the handler returns, so the CPU
re-enters immediately, forever. Legacy PCI interrupt pins are level-triggered
and shared, which is the combination that makes a single unserviced function
able to halt the system.

On this tree's QEMU configuration IRQ 10 carries eight functions:

| Function | Device | Serviced by `handle_device_irq`? |
|---|---|---|
| `00:04.0` | virtio-net | yes |
| `00:05.0` | virtio-blk (disk 0) | yes |
| `00:08.0` | ATI Rage VGA | no |
| `00:09.0` | AC'97 audio | no |
| `00:0c.0` | NVMe | no |
| `00:0d.0` | xHCI USB | no |
| `00:1f.2` | AHCI SATA | no |
| `00:1f.3` | SMBus | no |

### Confirmed by observation: three offenders across two lines

The sweep samples Status bit 3 *before* silencing each function, so the boot log
answers the question the original entry could not. On the first boot carrying the
fix:

```
[pci] INTx disabled on 00:02.0 (8086:10d3, irq 11)
[pci] INTx disabled on 00:07.0 (1af4:1050, irq 11) — WAS ASSERTING with no handler
[pci] INTx disabled on 00:08.0 (1002:5159, irq 10)
[pci] INTx disabled on 00:09.0 (8086:2415, irq 10)
[pci] INTx disabled on 00:0a.0 (8086:2668, irq 11)
[pci] INTx disabled on 00:0b.0 (1af4:1059, irq 11) — WAS ASSERTING with no handler
[pci] INTx disabled on 00:0c.0 (1b36:0010, irq 10)
[pci] INTx disabled on 00:0d.0 (1b36:000d, irq 10) — WAS ASSERTING with no handler
[pci] INTx disabled on 00:1f.2 (8086:2922, irq 10)
[pci] INTx disabled on 00:1f.3 (8086:2930, irq 10)
[pci] INTx quiesce: 10 unclaimed function(s) silenced
```

`00:0d.0` is xHCI, which confirms the diagnosis below directly rather than by
argument. But **two of the three offenders are on IRQ 11, not IRQ 10**:
`00:07.0` virtio-gpu and `00:0b.0` virtio-sound. Both are polled drivers that
leave the virtio queue's interrupt-suppression flag clear, so every completion
asserts a pin nobody services — the same defect as xHCI, on a line that had
simply not yet been unlucky enough to storm.

That is the entry's main lesson. Had the fix been the narrow one this entry
originally proposed — find the IRQ 10 offender and give it a stub handler — IRQ
11 would still be carrying two armed devices, and the next report would have read
identically with a different number in it.

The claim/sweep split also verified itself: of 17 functions, 3 have no routed
interrupt (`irq=255`) and 4 were claimed (rtl8139, virtio-net, and *both*
virtio-blk disks), leaving exactly the 10 above.

### The specific offender, and why it is not the whole story

`kernel/src/xhci.rs` is a **polled** driver — `poll_event` walks the event ring
and there is no xHCI ISR anywhere — yet at `xhci.rs:878-890` it enables
Interrupter 0 (`IMAN.IE`) and sets `USBCMD.INTE`. It then never clears
`IMAN.IP` or `USBSTS.EINT`, the two write-1-to-clear bits that deassert the pin.
There is a live USB keyboard on an interrupt endpoint (`[xhci] Configured
keyboard on slot 1 (EP1 IN, 8 bytes, interval=7)`), so events arrive
continuously. A polled driver asking the hardware to raise interrupts nobody
handles is a bug on its own terms.

But fixing only xHCI would have been fixing the instance and not the class.
`ac97.rs` declares `CR_IOCE` and never writes it; `ahci.rs` declares `GHC_IE`
and `PORT_IE` and never writes them; `nvme.rs` passes `IEN=0`. Every one of
those is one line away from the same defect, and the next driver added to a
shared line starts from the same position. The IRQ 11 pair above is that
prediction already come true — written before the evidence arrived, and confirmed
by it.

**Follow-up, not yet done:** the sweep silences virtio-gpu and virtio-sound from
the outside, which stops the storm but leaves the underlying driver bug in place
— a polled virtio driver should be setting `VRING_AVAIL_F_NO_INTERRUPT` on its
queues so the device never asserts in the first place. Both drivers
(`kernel/src/virtio/gpu.rs`, `kernel/src/virtio/sound.rs`) are lane A's, so this
is lane A's to fix. Tracked here rather than in `todo.txt` because the symptom is
already contained and the fix is a cleanup, not a repair.

### The fix: a function nobody services may not assert

PCI 2.3 Command bit 10, **Interrupt Disable**, forbids a function from driving
its pin. It is one bit with identical meaning on every conforming function,
which is what makes it the right tool — the alternative, a per-device stub
handler that acks, requires correct knowledge of a *different* status register
for each of AC'97, NVMe, xHCI and AHCI, and a stub that guesses wrong leaves the
storm in place while looking like a fix.

Implemented in `kernel/src/pci.rs` as a claim/sweep pair:

- `pci::claim_intx(addr)` — called by the three drivers `handle_device_irq`
  actually dispatches to (virtio-blk, virtio-net, rtl8139), at the point each
  already calls `enable_bus_master`.
- `pci::quiesce_unclaimed_intx()` — called from `main.rs` immediately after
  `xhci::init`, the last PCI driver init. Disables INTx on every enumerated
  function that no driver claimed, and logs each one, noting whether it was
  asserting at the time.

The two are **order-independent**: a claim arriving after the sweep re-enables
its own function. That is deliberate, because an ordering requirement between a
sweep and every present and future driver init is exactly the kind of invariant
that holds until someone adds a driver and then fails silently.

### The diagnostic, which is the durable half

Status bit 3 (Interrupt Status) reports whether a function is asserting, is
read-only, and is *independent of* the Interrupt Disable bit — so it still names
a culprit after that culprit has been silenced. `irq_storm.rs` now calls
`pci::report_intx_asserting_on_irq` when it masks a line.

Before this, a storm reported only `[irq-storm] IRQ 10 MASKED: ~500000 IRQs/sec`
— which on a line with eight functions narrows the culprit to eight suspects.
That is precisely why the previous entry closed with "needs its own
investigation" rather than a fix. The next storm on any line names its own
device.

The report is deliberately **allocation-free** (`pci::for_each_function` rather
than `scan_bus0`): it runs from the timer softirq while a device is melting the
line, and a failed allocation in a kernel is a panic. A diagnostic that can turn
a recoverable storm into a dead machine is worse than no diagnostic.

### Two unrelated defects found in the same register

- **`enable_bus_master` erased recorded bus faults.** It read the
  Command/Status dword and wrote it back. Status' error bits (Master Data Parity
  Error, Signalled Target Abort, Received Master Abort, …) are
  **write-1-to-clear**, so writing back a read value sets a 1 into every bit that
  was already 1 — clearing exactly the errors that had been recorded. Enabling
  DMA destroyed the evidence of a bus fault. Fixed by `pci::write_command`,
  which writes **zero** into the status half; 0 is a no-op for a write-1-to-clear
  bit. `pci::self_test` asserts the invariant across every function on the bus,
  and reports how many carried a write-1-to-clear bit at the time so the log says
  whether the check was real or vacuous on that boot.
- **Clippy was error-level red on the tree**, on lane A's own
  `ex2_copy_plan` (`manual_is_multiple_of`, `proc/spawn.rs:481`). Anything
  running `cargo clippy -p kernel` got a hard failure unrelated to its own
  change. Fixed.

### Test

`pci::self_test_intx`, run from `pci::self_test` on every boot, before any
driver has claimed anything and before the sweep — so it is free to toggle bits
provided it restores them:

1. The claim key is injective over bus/device/function and never collides with
   the empty-slot sentinel. A collision would make one function's claim silently
   protect a *different* function; the sentinel case would make an empty slot
   read as a claim. Both fail in the direction of leaving a storm in place.
2. Enable/disable is observable and reversible on every function on the bus, and
   the Status register's write-1-to-clear bits survive the Command write.
   Functions that hardwire Interrupt Disable (QEMU's host bridge does) are
   tolerated and not counted; the test fails if *no* function accepts a toggle.
3. The claim table is empty before any driver initialises.
