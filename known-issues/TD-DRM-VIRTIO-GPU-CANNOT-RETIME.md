## TD-DRM-VIRTIO-GPU-CANNOT-RETIME (lane A, 2026-08-21)

**In short:** in a virtual machine, SlateOS cannot change the screen resolution.
It now says so with an error instead of quietly showing a cropped picture, which
is the important half — but the resolution still cannot actually be changed.

**Where.** `VirtioGpuBackend::set_mode`, `kernel/src/drm/driver.rs`. It returns
`InvalidArgument` unless `mode.hdisplay == self.width && mode.vdisplay ==
self.height`.

**Why.** The scanout resource is created once during probe, sized from
`GET_DISPLAY_INFO`, and never replaced. Everything downstream — the transfer
rectangle, the flush region, the pitch — is derived from that one size.

**Proper fix.** A resource-recreate path: `RESOURCE_CREATE_2D` at the new size,
`RESOURCE_ATTACH_BACKING`, `SET_SCANOUT` to point the scanout at it, then
release the old resource — with the old one kept alive until the new one is
scanning out, so a failure half-way leaves a working display rather than a
black one. `set_mode` is the entry point that gains it and nothing else has to
change; the object-model bookkeeping in `DrmDevice::set_crtc` is already
backend-agnostic.

**Severity.** Low, and it only became visible today. Before `SETCRTC` existed
there was no way to ask for a different mode at all, so nothing could be
refused. This is the honest report of a limitation that was previously hidden
behind a silent crop.
