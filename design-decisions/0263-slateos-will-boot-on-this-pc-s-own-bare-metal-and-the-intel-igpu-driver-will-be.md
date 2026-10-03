## §263 — SlateOS will boot on this PC's own bare metal, and the Intel iGPU driver will be written against the real chip

**Date:** 2026-08-21
**Decided by:** Operator (Claude recommended A-for-now with D over C; operator
chose C)
**Lane:** A

**In short:** Most laptops and many desktops display through Intel graphics
built into the CPU. We can't test a driver for it in the emulator — but unlike
the AMD case, *the chip is already inside this PC*, merely switched off in the
firmware settings, so no purchase is needed. The decision is to switch it on,
get SlateOS starting up on the real machine from a USB stick, and then write the
Intel driver against real silicon. The operator will do the physical half:
enable the iGPU in firmware, move the monitor cable to the Intel output, and
plug in a flash drive.

### Why the operator's choice overrides the recommendation, and why that is right

The recommendation was A-for-now, and D (a Linux host with GVT-g/VFIO
passthrough) over C if real support were ever wanted — on the grounds that C's
prerequisite, *booting a real PC*, is a substantially bigger job than the driver
it enables, and that D keeps testing **automated** rather than manual, which is
what the rest of this project's testing depends on.

Both of those remain true and neither was disputed. What they undervalue is that
bare-metal boot is a goal in the plan **in its own right**, not merely a means to
this driver. Charged against the driver alone it looks disproportionate; charged
against everything it unblocks — real storage, real USB, real ACPI, a real
monitor's EDID, a real accelerator for the benchmark suite (Q53 option D, Q54) —
it is the highest-leverage prerequisite on the board. D would have bought one
driver and left every one of those still unreachable.

### What was measured, not assumed

| Fact | Evidence |
|---|---|
| This PC's CPU contains Intel UHD Graphics 630 | CPU is an `Intel(R) Core(TM) i7-8700K`; that model includes UHD 630, a mainstream well-documented i915 target |
| The chip is switched off, not absent | Windows lists only `NVIDIA GeForce RTX 4090`; **no** Intel display device enumerates on the PCI bus at all — the usual state when a separate card is fitted. Re-enabling is a firmware toggle |
| Passthrough is unavailable on this host regardless | QEMU here accelerates via WHPX, which has no device passthrough. VFIO and Intel GVT-g are Linux-only — which is why option D needed a second OS |
| SlateOS has never run on real hardware | Every boot to date is QEMU. **This, not the chip, is the actual gate** |

### The risk, and its containment

The one real hazard is a black screen on the machine the operator works at. It
is contained by construction: boot from a **USB stick**, leave the Windows disk
untouched, and keep the NVIDIA output available to fall back to. Nothing about
this modifies the host installation.

### Sequencing

This is blocked on the operator being physically present, and they have said so
("whenever you're ready, just wait for me"). Lane A's job is to have the
bootable-USB path ready *before* asking: an image that a firmware will start, a
serial or on-screen channel that reports progress, and a documented recovery
step. The driver itself is the small half and comes last.
