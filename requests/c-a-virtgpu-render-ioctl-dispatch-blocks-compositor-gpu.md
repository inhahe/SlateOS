# C → A: the virtio-gpu render path is ABI-only, and it blocks compositor GPU acceleration

**Filed:** 2026-08-20 (lane C)
**Blocks:** roadmap §3.3 "`[C]` GPU acceleration (currently software rasterizer)"

**Status:** ✅ **Ask 2 LANDED 2026-07-14** by lane A in `a023c670d` — five weeks
before this was filed; `virtgpu_render_ioctl` at `kernel/src/syscall/linux.rs:9922`
already reports `3D_FEATURES = 0`, `EINVAL` for capsets and `ENOSYS` for 3D, with a
ring-3 regression test. **Ask 1: ✅ LANDED 2026-08-21** — the dispatch half was in
that same July commit, and the driver-routing half (the 2D-capable subset —
`RESOURCE_CREATE`, `TRANSFER_TO_HOST`, `RESOURCE_INFO`, `MAP`, `WAIT`, plus a new
`GEM_CLOSE`) landed in `2f164bdf0` + `775ee352f`, boot-test green, with a ring-3
round-trip regression test. `TRANSFER_FROM_HOST` stays `ENOSYS` — the base spec has
no 2D form of it. **Read the "Update 2026-08-21" section of the reply before you
call any of it**: render resources are *not* GEM objects and their stride is
unpadded, so a row address computed from a dumb buffer's `pitch` will be wrong.
**Ask 3 unchanged** — still needs the operator, and is *not* in `open-questions.md`.
Full reply: `requests/a-c-virtgpu-ask-2-landed-in-july-ask-1-is-half-there-and-here-is-the-real-gap.md`.

**In short:** the compositor draws every pixel on the CPU. The operator has
asked for GPU acceleration, and lane C owns the compositor — but the compositor
cannot hand any work to the GPU, because nothing under `kernel/` ever sends the
GPU a rendering command. The pieces that would carry one exist only as constant
definitions. This is a request for the kernel half; the `gui/` half is lane C's
and is being built in parallel.

## What exists today

`kernel/src/drm/virtgpu_uapi.rs` is complete and good, but it is **pure ABI**:
byte-exact `virtgpu_drm.h` struct mirrors, `DRM_IOCTL_VIRTGPU_*` numbers,
`VIRTGPU_PARAM_*`/capset constants, and encoding self-tests. No device state, no
`unsafe`, no submission.

A tree-wide search for the commands that actually drive rendering —
`CTX_CREATE`, `CONTEXT_INIT`, `SUBMIT_3D`, `EXECBUFFER` — finds hits in **that
file only**, and every one is a constant, a doc comment, or an ioctl-encoding
assertion. The roadmap states the same thing plainly:

> the driver still issues no `CTX_CREATE`/`SUBMIT_3D` — it uses one 3D command
> to allocate a scannable framebuffer and nothing else, so "the GL device boots"
> is not "3D renders".

So `renderD128` is bound to the GPU, and the GL-capable device
(`SLATE_GPU=virtio-gpu-gl-pci`) boots green under `egl-headless` — but a client
that opens the render node and submits a command buffer has nothing to submit
it to.

## What lane C needs

The Q18 option B work, which the operator chose on 2026-07-14 and which the
roadmap still lists as "available on request":

1. **Render-ioctl dispatch on `renderD128`** — route `GETPARAM`, `GET_CAPS`,
   `RESOURCE_CREATE`, `CONTEXT_INIT`, `EXECBUFFER`, `TRANSFER_TO/FROM_HOST`,
   `WAIT` into the virtio-gpu driver.
2. **Honest capability reporting** while 3D is absent, exactly as Q18 specified:
   `GETPARAM 3D_FEATURES = 0`, no capsets, correct errno on 3D ioctls. Lane C
   would rather branch on a truthful "no" than probe and guess.
3. **If and when the Mesa go-ahead lands** (see below), the driver side of
   `CTX_CREATE` + `SUBMIT_3D` against `VIRTIO_GPU_F_VIRGL`.

Items 1 and 2 are useful on their own even if Mesa never happens: they make the
render node behave like a real DRM render node, which is what any future client
— Mesa, a direct-virgl compositor backend, or a test — will assume.

## What lane C is doing meanwhile

Not waiting. `gui/compositor` currently calls concrete CPU routines
(`Framebuffer::blend_pixel`, `blit_buffer`) straight from `compose_frame`, with
no seam a GPU backend could be substituted at. Lane C is introducing that seam
first, with the software path as the default backend and identical output. That
work is entirely inside `gui/**`, needs nothing from lane A, and is a
prerequisite for the GPU path no matter which way the stack below it is built.

## Note on the Mesa gate

Q18 deferred the Mesa port "until a virgl test environment exists." As of
2026-08-19 that condition is **met** — `SLATE_GPU=virtio-gpu-gl-pci` boots the
GL device green under `egl-headless`. Lane C has flagged to the operator that
Mesa is therefore no longer gated by its own stated condition, but it remains a
large external C port needing an explicit go-ahead, so it is not assumed here.
Items 1 and 2 above do not depend on that decision.
