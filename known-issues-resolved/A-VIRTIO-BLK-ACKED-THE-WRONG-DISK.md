### [A] A-VIRTIO-BLK-ACKED-THE-WRONG-DISK: with two virtio disks, IRQ 10's interrupts were acknowledged at the other disk, and the line stormed three or four times a boot -- 2026-09-27
**Status:** FIXED on lane-a 2026-09-27, awaiting a boot. Found chasing the
IRQ 10 storms in the rq10-rq12 boot logs, after rq12's `connect6` failure.

**In short:** the swap disk and the network card share one interrupt wire.
When the swap disk finished a request, the kernel "answered" it by checking
the *other* disk, so the swap disk never let go of the wire, and the CPU was
interrupted about half a million times a second until a safety check shut the
wire off for ten seconds. Then it happened again. Network tests running in
those seconds timed out. The other disk's wire was never switched on at all,
so its requests finished only when something else woke the CPU.

**Where.** `kernel/src/virtio/blk.rs` kept one I/O port and one IRQ line in
two globals for every virtio-blk device:
- `BLK_IO_BASE` was written by each device's `init`, so the last device won:
  the rootfs disk (`00:06.0`, port `0x6700`, IRQ 11).
- `BLK_IRQ_LINE` was written by `probe_all` for the first device only: the
  swap disk (`00:05.0`, IRQ 10).

`handle_irq(10)` therefore read the ISR status register at `0x6700`, the
rootfs disk's. The swap disk's register was never read, so its
level-triggered pin stayed asserted. `enable_interrupts` unmasked only
`BLK_IRQ_LINE`, so IRQ 11 stayed masked.

**Evidence.** The storm diagnostic added by
`A-IRQ10-STORM-IS-AN-UNHANDLED-DEVICE-ALLOWED-TO-ASSERT` named the culprit on
every storm:

    [irq-storm] IRQ 10 MASKED: 530000 IRQs/sec for 3s (storm #1, cooldown 10s)
    [pci]   irq 10 asserted by 00:05.0 (1af4:1001) [claimed]

"Claimed" is what made this a different bug from that entry's. Its fix
silenced every function no driver serviced. This function had a driver; the
driver read the wrong port.

In rq12 the second storm covered `persistent netstack connect6` (TimedOut)
and the ring-3 HTTP capstone (no exit within 15 s), back to back. In rq11
and the two boots before it, the storm was masked before `connect6` ran, and
both passed. That is a correlation over four boots, not a proof; the boot
after this fix will say whether `connect6` has another cause.

**The fix.**
- Each device records its own route in `IRQ_ROUTES`: one `AtomicU64` per
  device, holding its line, its port and its PCI address. A reader can never
  pair one device's line with another's port.
- `handle_irq` acknowledges every function routed to the line.
- `enable_interrupts` unmasks every line a disk uses.
- `init` registers the route before claiming the function's INTx. A function
  it cannot route (the table is full) stays unclaimed, and
  `pci::quiesce_unclaimed_intx` silences it.
- `VirtioBlkDevice::enable_irq` is deleted. It had no caller, and unmasked a
  line without routing it.

**Test.** `virtio::blk::self_test_irq_routes` runs straight after
`enable_interrupts`:
1. Each route must match its function's configuration space: the IRQ line
   and the BAR0 port.
2. For every registered virtio-blk disk, one sector is read, and the
   function's INTx status bit (PCI Status bit 3) must clear within 200 ms.

Before the fix, both disks fail step 2: the swap disk because its line was
acknowledged at the other port, the rootfs disk because its line was never
unmasked. A routed disk missing from the registry fails the test by name,
rather than passing unexercised.
