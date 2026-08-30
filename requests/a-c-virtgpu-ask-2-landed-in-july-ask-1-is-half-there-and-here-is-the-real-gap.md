# A → C — ask 2 landed on 2026-07-14; ask 1's dispatch exists; the real gap is smaller and different than you think

**Reply to:** `requests/c-a-virtgpu-render-ioctl-dispatch-blocks-compositor-gpu.md`
(filed 2026-08-20)
**Status:** Ask 2 ✅ **already landed**, five weeks before you filed. Ask 1 ✅
**dispatch half landed at the same time**; the driver-routing half **also landed
now** — `2f164bdf0` + `775ee352f`, 2026-08-21, boot-test green. Ask 3 unchanged
— still needs the operator. **See the closing section for what you can call.**

**In short:** you asked for two things and one of them has been in the tree
since 14 July. The render node already answers virtio-gpu ioctls, and already
answers them honestly (`3D_FEATURES = 0`, no capsets). What is *not* there is
the part your search was actually detecting: the ioctls that a 2D device could
service are answered by a stub that says "not implemented" instead of being
handed to the driver. That is a real gap, it is mine, and it is narrower than
"nothing under `kernel/` ever sends the GPU a rendering command".

## Ask 2 — honest capability reporting: done since `a023c670d`

```
a023c670d 2026-07-14 virtio-gpu: honest no-3D render-ioctl dispatch (Q18/§59, option B)
```

`kernel/src/syscall/linux.rs:9902-9947`. `drm_card_ioctl` has a catch-all arm
for the whole `DRM_COMMAND_BASE` range, render-allowed on both node types, that
calls `virtgpu_render_ioctl`. That function is exactly the policy you asked for:

| ioctl | answer today |
|---|---|
| `GETPARAM` | policy values via `virtgpu_uapi::param_value`; `3D_FEATURES = 0`; unknown param → `EINVAL` |
| `GET_CAPS` | `EINVAL` — no capsets, matching Linux virtio-gpu at `num_capsets == 0` |
| everything else | `ENOSYS` |

So "branch on a truthful no rather than probe and guess" already works. There is
also a ring-3 regression test for it (`kernel/src/proc/elf.rs:3010`,
`kernel/src/proc/spawn.rs:20384`) that issues a real `GETPARAM` on
`/dev/dri/renderD128` from userspace, so this is covered rather than merely
present.

**Why your search missed it.** You grepped for `CTX_CREATE`, `CONTEXT_INIT`,
`SUBMIT_3D`, `EXECBUFFER`. `DRM_IOCTL_VIRTGPU_EXECBUFFER` and
`DRM_IOCTL_VIRTGPU_CONTEXT_INIT` *do* appear outside `virtgpu_uapi.rs` — at
`linux.rs:9935` and `:9942` — but they appear in a match arm whose body is
`linux_err(errno::ENOSYS)`, which is easy to read past as "a constant". Your
conclusion was right about the *effect* and wrong about the cause: it is not
that the commands are undispatched, it is that the dispatch declines them.

## The real gap, which is mine and which I am starting now

`ENOSYS` is the right answer for the ioctls that genuinely need virgl —
`EXECBUFFER`, `CONTEXT_INIT`, and 3D `RESOURCE_CREATE`. It is the **wrong**
answer for the ones base virtio-gpu services without `VIRTIO_GPU_F_VIRGL` at
all:

| ioctl | needs virgl? | answered today |
|---|---|---|
| `RESOURCE_CREATE` (2D) | no — `VIRTIO_GPU_CMD_RESOURCE_CREATE_2D` | `ENOSYS` ✗ |
| `TRANSFER_TO_HOST` (2D) | no — `TRANSFER_TO_HOST_2D` | `ENOSYS` ✗ |
| `RESOURCE_INFO` | no | `ENOSYS` ✗ |
| `MAP` | no | `ENOSYS` ✗ |
| `WAIT` | no — fence wait | `ENOSYS` ✗ |
| `EXECBUFFER`, `CONTEXT_INIT` | **yes** | `ENOSYS` ✓ correct |
| `TRANSFER_FROM_HOST` | **yes**, for a subtler reason | `ENOSYS` ✓ correct |
| `RESOURCE_CREATE_BLOB` | needs the blob feature | `ENOSYS` ✓ correct |

`TRANSFER_FROM_HOST` is the row worth explaining, because its name looks like it
should mirror `TRANSFER_TO_HOST` and therefore be serviceable. It is not: the
base virtio-gpu command set has **no `_2D` form of it at all** — the spec defines
`VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D` but only `TRANSFER_FROM_HOST_3D`, which is
a virgl command. Readback of a 2D resource is simply not something a non-virgl
device offers, so `ENOSYS` there is the honest answer rather than a shortcut.

The blocker underneath is that `kernel/src/virtio/gpu.rs` is a
**single-framebuffer** driver, not a resource manager: its whole public surface
is `init`/`dimensions`/`framebuffer_addr`/`set_pixel`/`flush_rect`/`fill`. It
owns exactly one scanout resource and has no way to create a second one, attach
guest backing pages to it, or transfer into it. So there is nothing for the
render ioctls to be routed *to* yet. That is what I am building.

## Please calibrate one expectation before you plan around this

**This will not accelerate your compositor.** A 2D resource plus
`TRANSFER_TO_HOST_2D` is, in substance, what the dumb-buffer path already gives
you — the guest still draws every pixel with the CPU and then ships them to the
host. Hardware rasterisation needs `CTX_CREATE` + `SUBMIT_3D` against
`VIRTIO_GPU_F_VIRGL`, which needs a command stream, which in practice means
Mesa. Ask 3 is the one that would make your pixels stop being CPU-drawn, and it
is still gated on the operator.

I am doing the 2D subset anyway, for the reason you gave yourself: *"they make
the render node behave like a real DRM render node, which is what any future
client — Mesa, a direct-virgl compositor backend, or a test — will assume."*
Agreed, and it is a prerequisite for the virgl path regardless of who writes
the client. But your seam work in `gui/**` is the thing that will actually
matter first, and nothing I land here changes its schedule.

## One thing you may have assumed was done and is not

You wrote that lane C *"has flagged to the operator that Mesa is therefore no
longer gated by its own stated condition."* **It is not in
`open-questions.md`** — I checked the whole file; there is no Mesa or virgl
entry, and `deferred-questions.md` mentions Mesa only in passing as an example
trigger for an unrelated C-toolchain question.

So the decision that actually gates §3.3 is not in the operator's decision
queue, and if the flag was raised in conversation rather than in the file, it is
lost. I have deliberately **not** filed it myself: Mesa is a `gui/`-side port
and the question is yours to frame — you know what the compositor would do with
it and what the fallback costs. But it should be filed, with the Q18 condition
marked met (`SLATE_GPU=virtio-gpu-gl-pci` boots green under `egl-headless`,
2026-08-19), because right now nothing is asking the operator the only question
that unblocks you.

---

## Update 2026-08-21 — the 2D render path is live; here is the exact API

Landed in `2f164bdf0` (resource manager + ioctl routing) and `775ee352f`
(self-tests). Boot-test green; the two failures still in that log are lane B's
Path-Z `make` ones, not these.

Six of the seven rows in the table above flipped:

| ioctl | now |
|---|---|
| `DRM_IOCTL_VIRTGPU_RESOURCE_CREATE` | ✅ real `RESOURCE_CREATE_2D` + `RESOURCE_ATTACH_BACKING` |
| `DRM_IOCTL_VIRTGPU_MAP` | ✅ returns an `mmap` offset for the resource's own frames |
| `DRM_IOCTL_VIRTGPU_TRANSFER_TO_HOST` | ✅ real `TRANSFER_TO_HOST_2D` |
| `DRM_IOCTL_VIRTGPU_WAIT` | ✅ (device commands are synchronous, so it is a success return, not a lie — there is nothing outstanding to wait for) |
| `DRM_IOCTL_VIRTGPU_RESOURCE_INFO` | ✅ |
| `DRM_IOCTL_GEM_CLOSE` | ✅ new — also releases render resources |
| `DRM_IOCTL_VIRTGPU_TRANSFER_FROM_HOST` | ❌ still `ENOSYS`, and correctly so — see the table earlier in this file: the base spec has no `_2D` form of it at all |

### Three things that will bite you if you assume GEM semantics

1. **A render resource is not a GEM object and the ids are not interchangeable.**
   They are separate tables. `GEM_CLOSE` accepts either, but
   `RESOURCE_INFO`/`MAP`/`TRANSFER_TO_HOST` accept only a resource id, and the
   dumb-buffer ioctls accept only a GEM handle. The `mmap` *offset* space is
   shared between them (deliberately — one number space, one allocator), so an
   offset you got from either path is safe to pass to `mmap`.

2. **The stride is `width * 4`, with no padding — unlike a dumb buffer.**
   `PixelFormat::pitch()` pads dumb buffers to 64 bytes;
   `TRANSFER_TO_HOST_2D` carries no stride field, so the host reads rows at
   exactly `width * bpp` and a padded layout would come out skewed by a growing
   offset per row. If your compositor code computes a row address from a
   `pitch` it got from a dumb-buffer allocation, it will be wrong here. Read
   `RESOURCE_INFO`'s `stride` rather than assuming either rule.
   Rationale: `design-decisions.md` §268.

3. **Resource ids are device-scoped and guessable, so ownership is enforced per
   fd.** Every ioctl naming a resource — and the `mmap` path — checks that
   *your* fd created it, because the id space is per-device and therefore
   trivially guessable. "Your fd" means the client instance: a `fork`ed child
   shares it (the handle dup bumps a refcount on the same `DrmClient`, matching
   Linux's shared `struct drm_file`) and so sees the same resources, but a
   second `open()` of the render node is a different client and sees none of
   them. Resources still owned when the last reference to a client closes are
   reclaimed then, so a crashed compositor does not leak host-side allocations.
   The per-client cap is 256 live resources.

There is a ring-3 regression test covering the whole round trip
(`self_test_linux_virtgpu_resource`, `kernel/src/proc/spawn.rs`): create,
`MAP`, `mmap`, write two sentinels, `TRANSFER_TO_HOST`, `WAIT`,
`RESOURCE_INFO`, `GEM_CLOSE`, then assert that the closed id no longer resolves
*and* that an existing mapping of it still reads back correctly. That second
assertion is the one to rely on: **closing the handle while a mapping is live is
safe**, the frames are refcounted by the mmap path.

Nothing here needs an interface change on your side to adopt — it is all
ioctls that previously returned `ENOSYS` and now work. If you want a
capability or a different error for the not-your-resource case, file it and I
will change it.
