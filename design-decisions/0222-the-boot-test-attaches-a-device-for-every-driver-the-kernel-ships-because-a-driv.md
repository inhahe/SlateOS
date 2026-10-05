## §222 — The boot test attaches a device for every driver the kernel ships, because a driver whose hardware is absent is not lightly tested, it is untested

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The automated test boots the OS inside a simulated PC. That
simulated PC had a disk, a network card and two graphics cards — and no sound
card of any kind, even though the kernel ships three sound drivers. Those
drivers had therefore never run a single line past "is there a device?" on any
of the twenty boot tests before this one; they were being changed and declared
correct on the strength of the compiler alone. The decision is to attach a
device for every driver the kernel has, and to accept the boot test getting
slower and noisier in exchange. Attaching the three sound cards cost **2
seconds** and found **two real bugs within one boot**.

### The alternatives

| | **Attach the hardware** (chosen) | **Leave it out** |
|---|---|---|
| *What changes:* | every driver executes on every boot; boot test grows by seconds per device | boot test stays minimal; driver code is verified by reading it |
| Cost | boot time, log noise, occasional device-model interactions | a whole class of code that no test can reach |

The case for leaving it out is not empty, and it is worth stating honestly:
each device adds boot time, adds pages of serial output a human has to skim,
and occasionally perturbs the rest of the machine (`ati-vga` already
demonstrates this — it is a VGA device and suppresses others on the bus). A
test that grows without bound eventually stops being run.

### Why the coverage wins anyway

The precedent was already in the script, written for the graphics card:

> It is here so the ATI driver's register offsets are checked against a device
> on every boot instead of being trusted.

That reasoning is not specific to graphics. It says: a register offset, a reset
sequence, a DMA descriptor layout — these are *claims about hardware*, and the
only thing that can refute a claim about hardware is hardware. Code review
cannot, because the reviewer and the author are consulting the same
possibly-misread datasheet. This is sharper for an AI-written kernel than for a
human one: the failure mode here is not carelessness but confident recall of a
register semantic that is subtly wrong, and confident wrong recall survives
review by construction.

The measured exchange rate settles it. Two seconds of boot time (345 s vs
343 s) bought, on the very first run:

- **AC97 was programming its DMA base address and then erasing it** one line
  later, by resetting the channel afterwards rather than before — the card was
  left pointed at physical address 0.
- **virtio-sound was bound to a transport the device has never implemented**,
  reporting "BAR0 is not I/O space" in a way indistinguishable from "no sound
  card present" (see §223).
- First-ever execution of the HDA codec probe, the audio-function-group walk
  and output-stream configuration.

Two bugs per two seconds is not a close call, and both were of the kind that
ships: neither produces a compile error, neither is visible in a diff, and both
would have surfaced on real hardware as "audio doesn't work" with no diagnostic.

### The rule this generalises to

**Before restructuring a driver, check whether the boot test attaches its
hardware. If it does not, attach it first and let the boot test tell you what
is broken.** A driver whose device is absent is untested — not lightly tested,
not covered-by-inspection, untested — and the correct response to discovering
that is to fix the test bed, not to review the driver harder.

Three configuration details are load-bearing and are recorded in
`scripts/boot-test.sh`'s comment block so nobody "simplifies" them away:

- **`audiodev=none` is the right backend, not a cop-out.** It discards the
  samples but leaves the device *model* — register file, DMA engine, descriptor
  walk — fully intact. The model is what is under test; the host's speakers are
  not.
- **`hda-duplex` is mandatory, not decoration.** `intel-hda` alone is a
  controller with no codec attached, so `STATESTS` reads 0, the driver correctly
  concludes there is nothing to talk to, and the entire CORB/RIRB verb
  round-trip — the part most likely to be wrong — stays untested. A device that
  is present but inert reproduces the exact gap this entry exists to close,
  while *looking* like coverage.
- **Audio devices are multimedia-class**, so unlike `ati-vga` they suppress
  nothing else on the bus.
