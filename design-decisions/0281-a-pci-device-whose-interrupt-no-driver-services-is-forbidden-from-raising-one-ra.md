## §281 — A PCI device whose interrupt no driver services is forbidden from raising one, rather than being given a stub handler that swallows it

**Date:** 2026-08-22
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Several devices share one "please look at me" wire to the CPU.
That wire is held down until whichever device pulled it lets go, and only that
device's own driver knows the trick to make it let go. Six devices on this wire
have drivers that never listen to it, so if one pulled the wire nothing released
it and the CPU answered forever — half a million times a second. There were two
ways to fix it: teach the kernel the release trick for each of those devices, or
tell any device nobody listens for that it may not pull the wire at all. The
second was chosen.

**The choice was validated on the first boot that carried it.** The generic fix
logs what it silences, and it found *three* devices holding a wire down with
nobody listening — but only one of them was on the wire the bug report named. The
other two sat on a second shared wire with the same fault, which had simply not
yet been unlucky. The device-by-device fix would have addressed one of the three
and left the other two armed. See "What the evidence said" below.

### The situation

Legacy PCI interrupt pins are **level-triggered and shared**. Level-triggered
means the condition is still true when the handler returns, so an unserviced
assertion re-enters the handler immediately and forever; shared means one IOAPIC
input carries several functions and any one of them can do this. On this tree's
QEMU configuration IRQ 10 carries eight functions and the ISR knows how to
quiesce three. A storm there was measured at ~500 kHz and starved the scheduler
badly enough to wedge a boot.

### Option A — a stub ISR per unhandled device

Give AC'97, NVMe, xHCI, AHCI (and any future arrival) a minimal handler that
reads whatever register that device uses to deassert.

*For:* keeps every device able to interrupt, so a driver that later grows a real
ISR needs no change to the interrupt plumbing.

*Against:* the deassert sequence is **different for every device**, and the
kernel has to be right about all of them. AC'97 needs a Status-register write,
NVMe deasserts on a completion-queue head doorbell, xHCI needs `IMAN.IP` *and*
`USBSTS.EINT`, AHCI needs a port Interrupt Status write *and* the global one. A
stub that guesses one of these wrong leaves the storm exactly as it was **while
looking like a fix** — and the symptom, a wedged boot with no message naming a
device, is the one that already cost a day of investigation. It also scales
badly in the wrong direction: every new device is new device-specific code in
the kernel whose only purpose is to service an interrupt nobody wants.

### Option B — forbid the assertion at the source (chosen)

PCI 2.3 Command bit 10, **Interrupt Disable**, forbids a function from driving
its pin at all. Drivers that genuinely service a function call
`pci::claim_intx`; `pci::quiesce_unclaimed_intx` silences everything else.

*For:* one bit, identical meaning on every conforming function — there is
nothing to get device-specifically wrong. It acts at the source, so the
interrupt is never generated rather than generated-and-swallowed. It is what
Linux does for devices not using INTx. And it is **correct on its own terms
independent of which device was storming**: a function whose interrupt nothing
services must not be permitted to assert one, whether or not it currently does.
That mattered here, because the storm is intermittent and a green boot does not
prove a hypothesis about which device causes it.

*Against:* a driver that acquires an ISR later must remember to claim, or its
device will be silent — and "silent device" is a confusing symptom. This is the
real cost and it is mitigated, not eliminated, below.

### What makes Option B safe: claims and the sweep are order-independent

The obvious implementation requires the sweep to run after every driver's init.
That is an invariant between one function and every present *and future* driver,
and it fails silently the first time someone adds a driver below the sweep.

So `claim_intx` checks whether the sweep already ran and, if so, re-enables the
function it names. Claim-before-sweep and claim-after-sweep reach the same state.
The placement in `main.rs` is then a matter of *when* devices get muzzled rather
than *whether*, which is a much weaker thing to get wrong. Claim-table overflow
logs loudly rather than dropping, for the same reason: an unrecorded claim
presents as a device that never responds.

### What the evidence said

The sweep samples Status bit 3 before silencing, so the first boot carrying the
fix reported which functions were actually asserting into a void:

| Function | Device | IRQ | Asserting? |
|---|---|---|---|
| `00:0d.0` | xHCI USB | 10 | **yes** |
| `00:07.0` | virtio-gpu | 11 | **yes** |
| `00:0b.0` | virtio-sound | 11 | **yes** |
| 7 others | ATI VGA, AC'97, NVMe, AHCI, SMBus, e1000, HDA | 10/11 | no |

Two points, both of which argue for the option chosen:

1. **The predicted offender was right, but incomplete.** xHCI was diagnosed by
   reading the source — a polled driver that sets `IMAN.IE` and never clears
   `IMAN.IP`. That held. But two-thirds of the actual offenders were on IRQ 11,
   a line nobody had complained about, for the same reason in a different
   driver. Option A is scoped by the bug report; Option B is scoped by the
   invariant, and only the invariant found these.
2. **A green boot proves nothing about a rare storm, but the sweep's log does.**
   The boot on which this landed did not storm. Under Option A there would have
   been no way to tell a correct stub from a wrong one. Under Option B the fix
   and the evidence are the same mechanism.

### Why the diagnostic was built even though the fix does not need it

Status bit 3 is read-only and independent of the disable bit, so a silenced
function still reports that it wanted to interrupt. Reporting it when a line is
masked converts "IRQ 10 is storming" — eight suspects — into a named function.
The previous `known-issues.md` entry closed with "needs its own investigation"
precisely because the log could not say which device it was; leaving that true
would mean fixing this instance and rediscovering the next one the same
expensive way.

The report is allocation-free on purpose. It runs from the timer softirq while a
device is melting the line, and a failed kernel allocation is a panic — a
diagnostic that can turn a recoverable storm into a dead machine is worse than
none.

### A smaller call inside the choice

`pci::write_command` writes **zero** into the Status half of the
Command/Status dword rather than reading it and writing it back. Status' error
bits are write-1-to-clear, so the read-modify-write that `enable_bus_master`
previously used cleared exactly the errors that had been recorded — enabling DMA
destroyed the evidence of a bus fault. Writing zero is a no-op for a
write-1-to-clear bit, so every recorded error survives. This was a pre-existing
bug in `enable_bus_master`, fixed here because it is the same register and the
same mistake, and because the INTx writes would otherwise have inherited it.
