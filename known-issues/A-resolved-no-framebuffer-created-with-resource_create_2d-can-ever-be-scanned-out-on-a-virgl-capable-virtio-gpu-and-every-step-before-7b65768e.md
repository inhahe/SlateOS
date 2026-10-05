### [A] RESOLVED — no framebuffer created with `RESOURCE_CREATE_2D` can ever be scanned out on a virgl-capable virtio-GPU, and every step before the last one reports success — 2026-08-19

**In short:** with QEMU's GL-capable GPU (`virtio-gpu-gl-pci`) our display
never came up: `SET_SCANOUT` was refused with `ERR_INVALID_RESOURCE_ID` for a
resource the device had just created successfully. It was not our bug in the
usual sense — OVMF's own driver fails identically on the same host — but it was
ours to fix, because the only way out is for the guest to create the
framebuffer with a *different command*. It now does, and the GL device brings
up the display. This was the kernel-side prerequisite that
`requests/a-c-the-virgl-test-environment-that-blocked-mesa-now-exists.md`
promised lane C.

**The chain, all four links needed:**

1. QEMU routes *every* command through virglrenderer as soon as the device is
   `virtio-gpu-gl`, regardless of feature negotiation. Declining
   `VIRTIO_GPU_F_VIRGL` does **not** keep you on the 2D path — we accepted no
   page-0 features at all and were still on it.
2. QEMU's `virgl_cmd_create_resource_2d` hardcodes `args.bind = (1 << 1)`
   (`RENDER_TARGET`). The 2D command has no `bind` field, so no guest can
   influence this.
3. virglrenderer's `vrend_resource_d3d_init` allocates the shared D3D11 texture
   only for a resource whose bind includes `VIRGL_RES_BIND_SCANOUT` (1 << 18).
   Without that bit it returns early, leaving `res->d3d_tex2d == NULL`.
4. On Windows, `virgl_renderer_resource_get_info_ext` — which QEMU calls on
   `SET_SCANOUT` — ends in `vrend_renderer_resource_d3d11_texture2d`, which
   returns `EINVAL` when there is no D3D11 texture. QEMU turns that into
   `ERR_INVALID_RESOURCE_ID`.

**The fix:** create the framebuffer with `RESOURCE_CREATE_3D` (0x0204) instead,
with `target = PIPE_TEXTURE_2D` and `bind = RENDER_TARGET | SAMPLER_VIEW |
SCANOUT`. That command carries an explicit `bind`, QEMU dispatches it
unconditionally, and it passes the guest's value through verbatim. It is used
only when the device offered `VIRTIO_GPU_F_VIRGL`; a plain `virtio-gpu-pci`
does not offer it and takes the unchanged 2D path. We now accept that bit —
not to render 3D, but because it is the device's statement that the 3D command
block exists, which is exactly the condition under which the 2D path is a dead
end.

**Why this took a debugger and not a code read.** The failure is structurally
invisible from inside the guest. QEMU's virgl path *discards* virglrenderer's
return code in `virgl_cmd_create_resource_2d`, and never sets `cmd->error` in
`virgl_resource_attach_backing` or `virgl_cmd_transfer_to_host_2d` — so create,
attach and transfer all answer `OK_NODATA` for a resource that cannot be
displayed, and `SET_SCANOUT`'s `0x1203` is the *only* guest-observable signal.
There is no command sequence a guest can issue to tell "the resource was never
created" from "the resource exists but has no shareable texture". Two
hypotheses were killed by experiment before the right one — the resource format
(`B8G8R8A8` → `R8G8B8A8` changed nothing) and virglrenderer failing to
initialise (it initialises fine: `gl_version 31 - es profile enabled` via ANGLE
under `-display egl-headless`; it is `sdl,gl=on` that fails outright with
`virgl could not be initialized: 22`). What settled it was breaking on the
virglrenderer exports under `cdb`: `virgl_renderer_init` is called with flags
`0x1102`, which includes `VIRGL_RENDERER_D3D11_SHARE_TEXTURE` — so link 3 is
live on this host — and `virgl_renderer_resource_get_info_ext` returns `0x16`
(`EINVAL`).

**A wrong opcode cost a whole cycle, and it failed *silently*.** The first
attempt sent 0x0201 for `RESOURCE_CREATE_3D`. That is `CTX_DESTROY`, which
answers `OK_NODATA` — so the driver logged a successful resource creation for a
resource that was never created, and the symptom was byte-identical to the
original bug. The 3D block starts at 0x0200 (`CTX_CREATE`); `RESOURCE_CREATE_3D`
is 0x0204. The same shape of silent failure guards the *size* of the request:
QEMU's `VIRTIO_GPU_FILL_CMD` returns without setting an error if the descriptor
is shorter than the struct, so a short command is accepted and ignored. That is
why `VirtioGpuResourceCreate3d`'s trailing `_padding` is documented as wire
format and its 72-byte size is a `const _: () = assert!(…)` — nothing else in
the file would catch it.

**The GL path now has harness coverage:** `SLATE_GPU=virtio-gpu-gl-pci
./scripts/boot-test.sh` swaps the device *and* the display backend
(`egl-headless`; QEMU refuses to start a GL device under `-display none`) and
marks the run an experiment, since a run on a different display backend is not
comparable on wall time with a tracking run. Passing the device through
`QEMU_EXTRA` cannot substitute: that *adds* a second GPU and the driver binds
the first one it finds, so the GL device is never the one under test — which is
how an earlier attempt to reproduce this silently measured the plain device.
