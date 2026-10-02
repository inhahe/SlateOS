## §225 — A driver self-test that only reads back registers the driver itself wrote is a test of the driver's memory, not of the hardware

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Each network card driver had a "self-test" that checked things
like "is the MAC address sensible" and "is the transmit-enable bit set". Every
one of those values is something the driver had put there itself a moment
earlier, so the test passes even if the card is not actually able to send
anything. The decision is that each network driver's self-test now **actually
transmits a frame** and waits for the card to report it finished — which needs
no other machine on the network, because the card reports completion by writing
back into our own memory. Doing this immediately found two real bugs in the
RTL8139 driver that had been there since it was written.

### What the tests were checking

The e1000's self-test is representative. Five checks, all green, all vacuous:

| Check | What it proves |
|---|---|
| MAC is not all-zeros/all-ones | the driver read *something* out of the EEPROM |
| link status readable | the MMIO window is mapped |
| STATUS register != `0xFFFF_FFFF` | the MMIO window is mapped |
| `RCTL.EN` set | the driver's own init wrote `RCTL` |
| `TCTL.EN` set | the driver's own init wrote `TCTL` |

A driver whose transmit-ring base address is a *virtual* address instead of a
physical one passes all five. So does one whose descriptor struct has a field
in the wrong place, and one whose doorbell write lands on the wrong register
offset. Those are the three most likely ways a DMA-ring driver is wrong, and
none of them is observable from anything on that list.

### The alternatives

| | **Transmit a real frame** (chosen) | **Read-back checks only** |
|---|---|---|
| *What changes:* | each NIC sends one 60-byte frame during boot and the log reports the descriptor coming back | boot emits no packets |
| Argument for | the failure modes that actually happen are the ones only a transmission can detect | a self-test with no side effects can run anywhere, including on a machine attached to a network someone cares about |

The objection to transmitting is real and worth stating: a self-test that emits
packets is a self-test that can be *noticed* by other machines, and "the kernel
self-test spammed the LAN" is a legitimate thing to not want.

What defuses it is that the frame can be made inert by construction rather than
by policy. It is addressed **to our own MAC, from our own MAC**, with EtherType
`0x88B5` — which IEEE Std 802 reserves for local experimental use, so no
protocol anywhere claims it. No other station treats it as addressed to itself;
nothing parses it. It is one 60-byte frame per NIC per boot, and it is the
minimum that can prove anything at all.

The other half of what defuses it: **no peer is required.** Every one of these
cards reports completion itself, by DMA-writing into memory we own — the e1000
sets the Descriptor Done bit in our descriptor, the RTL8139 sets OWN in its
status register, virtio publishes the chain head in the used ring. So the test
works on a machine with no network at all, which is what makes it usable in an
automated boot test rather than a thing someone runs by hand occasionally.

### What it found immediately

Writing the RTL8139 half required reading the OWN bit's semantics properly, and
the driver had them **backwards**. The header comment read "set by software to
start TX, cleared by hardware on completion". The RTL8139 inverts the usual
convention: `OWN == 1` means the *host* owns the descriptor (Linux names the
same bit `TxHostOwns` in `8139too.c`), and writing the byte count is what clears
it. Two consequences, both live since the driver was written:

- The pre-send wait tested `status & OWN == 0` and broke out of the loop on it
  — that is, it treated "the NIC is **currently DMA-ing this buffer**" as "the
  buffer is free", then `memcpy`'d the next frame over it. Corruption on the
  wire, invisible from this end.
- When the descriptor genuinely *was* free (`OWN == 1`) the loop instead spun
  its full 100 000 iterations and then proceeded anyway, because the timeout
  had no failure path. So the check cost real time and bought nothing.

Separately, the driver never padded short frames. Ethernet requires 60 bytes
before the FCS; the transmit buffers are recycled, so a 40-byte frame would have
had 20 bytes of *the previous frame* appended to it and DMA-ed onto the wire.
That is kernel memory disclosed to the network, and silent from this end.

Both are fixed, along with the timeout now returning `TimedOut` instead of
falling through, and a per-descriptor in-flight flag — needed because
`OWN == 0` cannot distinguish "the NIC is still working on this" from "this
descriptor has never been used", and without the distinction the first four
sends of every boot would each burn the full timeout.

### The corollary that generalises

**A test whose assertions read back values the code under test wrote is a test
of nothing but the code's memory.** The check has to close a loop through
something the code does not control — here, the hardware's own DMA write-back.
This is the same shape as §222 ("a driver whose hardware is absent is not
lightly tested, it is untested") one level in: §222 got the hardware attached,
and this one is about what you then *ask* it.

The tell is worth naming, because it is easy to spot in review once you look
for it: if every assertion in a test would still hold after commenting out the
hardware, the test does not test hardware.
