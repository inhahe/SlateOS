## §217 — The AMD GPU driver targets the 25-year-old R100/Rage 128, because it is the only AMD display engine this project can actually execute

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)
**Lane:** C

**In short:** The design spec asks for an AMD graphics driver. The obvious
reading is a modern one — the GCN/RDNA chips in machines people own today. But
there is no way to *run* such a driver here: the emulator we test in (QEMU) does
not pretend to be any modern AMD chip, and the development machine has an NVIDIA
card in it. So a modern driver would be written entirely by guesswork and never
once executed. QEMU *does* emulate two ancient AMD/ATI chips faithfully, so the
driver targets those instead. It is real code that really runs, and the display
part of the newer chips is a direct descendant of it, so this is groundwork for
a modern port rather than a detour away from one.

### Context

Roadmap §3.1 calls for an AMDGPU driver. Before writing any of it I checked what
could be tested, and the answer bounds the whole task:

| Route | Available? |
|---|---|
| QEMU emulating a GCN/RDNA part | No — QEMU emulates no modern AMD GPU at all. |
| Real AMD hardware on the dev machine | No — the machine has an NVIDIA RTX 4090. |
| PCI passthrough of an AMD card | No card to pass through. |
| QEMU `ati-vga` (Rage 128 Pro `0x5046`, RV100 `0x5159`) | **Yes** — real MMIO registers, real CRTC timing registers, EDID over i2c-ddc, hardware cursor, 2D acceleration. |

A modern AMDGPU port is therefore not merely hard to test; it is untestable by
any route available. Every register offset, every bitfield, every ordering
constraint in it would rest on documentation alone, and nothing would ever
contradict a mistake.

### Decision

Write the driver against the R100/Rage 128 display block, structured so the
arithmetic is separable from the bus access:

- `kernel/src/drm/ati/regs.rs` and `timing.rs` are **pure** — register packing
  and CRTC timing computation, no MMIO. These run in a boot-time self-test on
  every boot, on any machine, present hardware or not.
- MMIO probing and `DrmBackend` integration sit on top, validated against
  QEMU's `ati-vga` rather than by assertion.

### Alternatives considered

**Write a modern AMDGPU driver blind.** It is what the spec literally asks for,
and it would look like progress. Rejected: an unexecuted display driver is not a
driver. The failure mode is not that it has bugs — it is that nothing about it
is known, including whether it initialises at all, and the appearance of a
completed roadmap item would actively mislead. A modern port also needs
atombios parsing, a full power/clock-gating bring-up and firmware loading, none
of which can be stubbed honestly without hardware to say what the stub got wrong.

**Do no AMD driver and rely on virtio-gpu.** Honest, and virtio-gpu already
works. Rejected as the *whole* answer: it leaves the driver stack with no native
mode-setting path at all, so the CRTC timing arithmetic — the part that is hard,
and that every future native driver needs — stays unwritten and unexercised.

**Target Intel instead.** QEMU has no Intel GPU emulation either, so this trades
one untestable target for another.

### Consequences

- The arithmetic that decides *what* to write to a CRTC — 8-pixel character
  units, bias-by-one totals, split sync-start fields, character-denominated sync
  widths — is exercised on every boot and is shared with the later R300/R500
  parts. Writing it caught a real bug immediately: the sync-width field is
  denominated in characters, not pixels, and a pixel-denominated encoder
  overflows its 6-bit field on 640x480@60, the most universally supported mode
  there is. That bug was found by hand-computing the expected register values
  for the mode table — exactly the check a blind port cannot perform.
- Nothing here advances support for GPUs anyone currently owns. That gap is a
  question for the operator, not a decision for me to make silently; it is filed
  in `open-questions.md`.
- If AMD hardware or passthrough later becomes available, this layer is the
  foundation a modern port extends rather than work to throw away — the display
  block's structure is inherited, and the pure/MMIO split is what makes the new
  registers testable in turn.
