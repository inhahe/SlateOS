## §223 — One modern virtio transport, shared by every modern virtio driver, and it always accepts `VERSION_1` on the caller's behalf

**Date:** 2026-08-17
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Virtio devices — the paravirtual disk/network/graphics/sound
hardware a virtual machine offers — come in two incompatible flavours of "where
are your registers": an old one and a new one. The kernel had code for both,
but the new one was locked inside the graphics driver where no other driver
could reach it, so the sound driver was written against the old one — against a
device that has never supported the old one and never will. It could not have
worked on any machine. The fix is to lift the new transport into a module both
drivers share, accepting that a bug in it now breaks the display as well as the
sound.

### The two transports, and why picking wrong is so quiet

| | **Legacy** (0.9.5) | **Modern** (1.0+) |
|---|---|---|
| Where the registers live | a fixed block of I/O ports at BAR0 | four memory regions, each announced by a vendor-specific PCI capability |
| How you reach them | `in`/`out` instructions | mapped memory |

`virtio-blk` and `virtio-net` are **transitional**: standardised before 1.0 and
still exposing the legacy block, which is why the legacy transport works for
them and why its existence never looked like a problem. **virtio-gpu and
virtio-sound are modern-only** — no I/O BAR at all. A legacy driver bound to one
gets exactly as far as *"BAR0 is not I/O space"* and stops, which reads to a
human skimming a boot log as "no device attached". That is the entire failure:
not a crash, not a wrong value, a plausible-sounding absence.

### The alternatives

| | **Extract a shared module** (chosen) | **Copy it into `sound.rs`** |
|---|---|---|
| *What changes:* | one transport, one place to fix; a bug in it breaks graphics *and* sound | each driver's transport bug stays local to that driver |

The copy is genuinely safer in one narrow sense, and that sense is not
hypothetical: `gpu.rs` drives the display, and the refactor put ~400 lines of
its probe path under shared ownership. A regression there is not a quiet
degradation, it is a blank screen.

It loses anyway, because **the duplication was the bug's cause, not a side
issue**. The modern transport already existed and was already correct; the
sound driver used the wrong one purely because the right one was unreachable
from outside `gpu.rs`. A second copy would leave the next modern-only virtio
driver in exactly the position `sound.rs` was in. The regression risk is
answered by the thing that found the bug in the first place — both devices are
now attached to the boot test (§222), so any transport change is exercised
against real device models on every boot.

### Sharing it fixed a latent bug in the driver that was already working

`gpu.rs`'s open-coded negotiation wrote 0 to feature page 1, which clears
`VIRTIO_F_VERSION_1` (feature bit 32) — i.e. the driver announced itself as
*legacy* to a modern-only device. QEMU's virtio-gpu tolerates it. A device that
follows the specification would refuse `FEATURES_OK`, and the driver would fail
with no diagnostic beyond "features not accepted". So the decision is not merely
that the transport is shared, but that **`negotiate()` always sets
`VIRTIO_F_VERSION_1` itself rather than making each caller remember to** — a
mandatory bit no modern driver may omit is not a parameter, and a caller that
can get it wrong eventually will.

A third bug fell out of the same read: `sound.rs` probed PCI device ID `0x1058`
as a "legacy sound ID". 0x1058 is virtio device type **24, virtio-iommu**. Had a
virtio-iommu ever appeared on the bus, the sound driver would have bound to it
and driven an IOMMU as a sound card. Only `0x1059` (type 25) is probed now.

### The guardrail

`kernel/src/virtio/mod.rs`'s module documentation now opens with "Two
transports, and how to pick one", because the next person to hit this will hit
it as a *symptom*, not as a design question, and the symptom points the wrong
way:

> a driver that reports "BAR0 is not I/O space" against hardware that plainly
> exists is not looking at a missing device — it is looking at a modern one
> through the wrong transport.
