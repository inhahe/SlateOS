## A-VIRTIO-SOUND-WAS-WRITTEN-AGAINST-A-TRANSPORT-THE-DEVICE-HAS-NEVER-HAD (lane A, 2026-08-17) - **fixed**

**In short:** with a virtio sound card attached to the boot test for the first
time, the driver printed "BAR0 is not I/O space or is zero" and gave up - which
reads exactly like "no sound card is present". The card was present. The driver
was talking to it through the *old* virtio protocol, and virtio-sound is new
enough that it has never implemented the old protocol and never will. The
driver could not have worked on any machine, ever.

**Symptom** (first boot with `-device virtio-sound-pci`):

```
[virtio-snd] BAR0 is not I/O space or is zero
```

### Cause - two virtio transports, and the wrong one was picked

Virtio devices come in two flavours of "how do I find the registers":

| | **Legacy** (0.9.5) | **Modern** (1.0+) |
|---|---|---|
| Where the registers are | a fixed block of I/O ports at BAR0 | four MMIO regions, each described by a vendor-specific PCI capability |
| How you reach them | `in`/`out` instructions | mapped memory |

`virtio-blk` and `virtio-net` are **transitional** devices - standardised before
1.0 and still exposing the legacy block - which is why the legacy transport in
`kernel/src/virtio/mod.rs` works for them. **virtio-gpu and virtio-sound are
modern-only.** They have no I/O BAR at all, so a legacy driver gets exactly as
far as "BAR0 is not I/O space" and stops.

The modern transport *did* already exist in this tree - but it was private to
`kernel/src/virtio/gpu.rs`, ~400 lines of it, with no way for another driver to
reach it. `sound.rs` was written against the only transport it could see.

### Fix - extract the transport, don't patch the driver

New `kernel/src/virtio/modern.rs` holds the shared modern transport
(`ModernTransport::probe` / `negotiate` / `setup_queue` / `notify_queue` /
`read_device_config32`). `gpu.rs` was refactored onto it (1437 -> ~1030 lines,
its private copy deleted) and `sound.rs` ported to it.

Sharing it fixed a **second, latent bug in `gpu.rs`** that no test would have
caught: the open-coded feature negotiation there wrote 0 to feature page 1,
which clears `VIRTIO_F_VERSION_1` (bit 32) - i.e. the driver declared itself
*legacy* to a modern-only device. QEMU's virtio-gpu tolerates it; a device that
follows the spec would refuse `FEATURES_OK` and the driver would fail with no
diagnostic beyond "features not accepted". `ModernTransport::negotiate` now
always accepts `VIRTIO_F_VERSION_1`, in one place, for every caller.

And a **third**: `sound.rs` probed PCI device ID `0x1058` as a "legacy sound
ID". 0x1058 is virtio device type **24, virtio-iommu**. Had a virtio-iommu ever
appeared on the bus, the sound driver would have bound to it. Only `0x1059`
(type 25, sound) is probed now.

### Guardrail

`kernel/src/virtio/mod.rs`'s module doc now opens with a "Two transports, and
how to pick one" section stating the rule directly: a new driver should reach
for `modern` unless it has a specific reason to want legacy, and

> a driver that reports "BAR0 is not I/O space" against hardware that plainly
> exists is not looking at a missing device - it is looking at a modern one
> through the wrong transport.
