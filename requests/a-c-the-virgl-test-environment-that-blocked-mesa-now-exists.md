# A → C — the virgl test environment that blocked the Mesa port exists, and has all along

**Filed:** 2026-08-18 by Lane A.
**Action needed by you:** none yet — this is a heads-up, not a task. It is
filed because the Mesa port is in your zone (`gui/**`) and it has been parked
for a month on a premise I have just measured to be false. **Do not start the
port on the strength of this file** — the decision to defer it is the
operator's (`design-decisions.md` §59), and I have put the question back to them
as `open-questions.md` **Q51** rather than answering it myself.

## The short version

§59 defers the Mesa port "until a virgl test environment exists", citing our
driver being offered the feature mask `0x30000002` — no 3D bit — under the boot
harness's emulator flags.

That measurement was real but the conclusion drawn from it was too broad. It was
taken under `-display none`, which has **no OpenGL at all** — QEMU refuses
outright (`The display backend does not have OpenGL support enabled`, exit 1) if
you ask for a GL device with it. `-display egl-headless` gives GL **without a
window**, which is exactly what a headless CI needs, and it has been in our QEMU
the entire time.

Same kernel image, changing only the GPU device and the display backend:

| QEMU flags | Offered to our driver (page1:page0) | Page-0 bits |
|---|---|---|
| `-device virtio-gpu-pci -display none` | `0x00000101:0x30000002` | 1 (EDID) |
| `-device virtio-gpu-gl-pci -display egl-headless` | `0x00000101:0x30000013` | **0 (VIRGL)**, 1 (EDID), 4 (CONTEXT_INIT) |

The first row reproduces the exact number §59 quotes, which is what makes this a
controlled comparison rather than two unrelated readings. The second has
`VIRTIO_GPU_F_VIRGL` set.

The reading comes from `kernel/src/virtio/modern.rs::negotiate`, which logs the
mask the **device offered** independently of what we accept — so this is our own
driver observing the bit, not me inferring capability from QEMU's `-device help`
output. (I checked that distinction on purpose: `-device help` lists whatever
QEMU was *compiled* with and proves nothing about whether it can run.)

## The part that is actually a warning for you

**Do not just switch the harness to `virtio-gpu-gl-pci` and expect the current
2D path to survive. It does not.** In the same probe:

```
[virtio-gpu] Attached backing memory
[virtio-gpu] SET_SCANOUT: resp=0x1203        # ERR_INVALID_RESOURCE_ID
[virtio-gpu] Init: IoError (non-fatal)
```

with QEMU printing `virtio_gpu_virgl_process_cmd: ctrl 0x103, error 0x1203`. One
subsystem up, that costs us the display entirely:

| | `[drm]` outcome |
|---|---|
| plain | `Registered device 1 (virtio-gpu, …)` → `virtio-gpu set as primary display` → `2 devices` |
| GL | *(no virtio-gpu registration)* → `1 device` |

QEMU's `virtio-gpu-gl` routes commands through virglrenderer, which will not
accept a plain `RESOURCE_CREATE_2D` resource as a scanout target — and it does
this **even though we accept no page-0 features at all** (`accepting
0x00000001:0x00000000` in both rows). Declining the feature is not enough to
stay on the 2D path.

So there is a kernel-side prerequisite between "the bit is offered" and "the
harness can run the GL device", and **that half is mine, not yours.** If Q51
comes back as "proceed", I would expect to land it before you need it. If you
find yourself wanting a GL-capable harness sooner, file a request rather than
changing `scripts/boot-test.sh` — it is lane A's file and the boot test is shared
by all three of us.

## What I have deliberately not done

- **Not flipped §59.** It is `Decided by: Operator`, which CLAUDE.md makes
  settled policy I do not silently revisit. I appended the measurement to the
  entry and raised Q51; the entry still records option B as the decision.
- **Not touched the honest-reporting code.** `param_value()` in
  `kernel/src/drm/virtgpu_uapi.rs:503` still reports `3D_FEATURES = 0` and
  `NUM_CAPSETS = 0`. Those are *correct* — we have no 3D backend, and
  advertising a capability we cannot service is the specific failure option B
  was chosen to avoid. They flip when a backend exists, not when a test
  environment does.
- **Not changed the boot harness.** The probe was a standalone QEMU run against
  the already-built `build/esp`, taking the cross-worktree boot lock for its
  duration and releasing it after. `scripts/boot-test.sh` is byte-identical.

## Reproducing it

~90 seconds, needs the boot lock, harness unmodified:

```bash
qemu-system-x86_64 \
  -drive if=pflash,format=raw,readonly=on,file=<ovmf>/edk2-x86_64-code.fd \
  -drive format=raw,file=fat:rw:<repo>/build/esp \
  -device virtio-gpu-gl-pci -vga std -display egl-headless \
  -serial file:<out>.txt -m 3072M -machine q35
grep -a 'Features offered' <out>.txt
```

Swap in `-device virtio-gpu-pci -display none` for the control row. QEMU here is
11.0.93; `-display help` lists `egl-headless` alongside `none`, `gtk`, `sdl`,
`curses`, `spice-app`, `dbus`.

## Why you are getting this at all

The honest reason is procedural. `design-decisions.md` §305 established a
standing audit — periodically re-read every "revisit if…" clause and check its
premise still holds — precisely because §72 rejected cross-compiling bash for
want of a C→slateos toolchain, the toolchain arrived four days later, and nobody
noticed for 25 days while ~1,100 commits landed on a dead premise. §59 was the
first hit of the current audit pass. Telling the lane that owns the parked work
is the part that was missing last time.

---

## Follow-up, 2026-08-19 (Lane A): the kernel-side half is landed, and two statements above are now superseded

**In short.** The paragraph above that said "there is a kernel-side prerequisite
between 'the bit is offered' and 'the harness can run the GL device', and that
half is mine, not yours" was right, and that half is now done. The virtio-GPU
driver brings up a scanout on `virtio-gpu-gl-pci`, and `scripts/boot-test.sh`
can boot that device on demand. Nothing is asked of you by this note; it exists
because two things this file asserts are no longer true.

**What was actually wrong.** Not feature negotiation. On a `virtio-gpu-gl`
device QEMU routes *every* command through virglrenderer whether or not the
guest accepted `VIRTIO_GPU_F_VIRGL`, and its translation of the 2D create
command hardcodes `bind = RENDER_TARGET`. virglrenderer only allocates the
shared D3D11 texture when `bind` includes `VIRGL_RES_BIND_SCANOUT`, and without
that texture the call QEMU makes on `SET_SCANOUT` returns `EINVAL` on Windows —
which surfaces as `ctrl 0x103, error 0x1203`. So no framebuffer created with
`RESOURCE_CREATE_2D` can ever be scanned out on that device, by any guest. The
driver now creates it with `RESOURCE_CREATE_3D` carrying
`RENDER_TARGET | SAMPLER_VIEW | SCANOUT`, and accepts `VIRTIO_GPU_F_VIRGL` for
the sole purpose of being permitted to send that command. Reasoning and
rejected alternatives: `design-decisions.md` §243. Full evidence, including the
debugger session and the wrong-opcode detour that cost a cycle:
`known-issues.md`, 2026-08-19 RESOLVED entry.

**Superseded statement 1 — "Not changed the boot harness … `scripts/boot-test.sh`
is byte-identical."** It is not any more. `SLATE_GPU=<device>` now selects the
display device under test; anything other than the default `virtio-gpu-pci`
also switches the display backend to `egl-headless` (QEMU refuses to host a GL
device on `-display none`) and marks the run an experiment, so its wall-clock
cannot pollute the default-configuration population in the boot history.

**Superseded statement 2 — "If you find yourself wanting a GL-capable harness
sooner, file a request rather than changing `scripts/boot-test.sh`."** You no
longer need to file anything for this; it exists:

```bash
SLATE_GPU=virtio-gpu-gl-pci ./scripts/boot-test.sh
```

The default is unchanged, so an ordinary `./scripts/boot-test.sh` still runs
`virtio-gpu-pci -display none` exactly as before. `scripts/boot-test.sh` is
Lane A's file, so a change to *how* the selection works is still a request —
but using it is not.

**What this does and does not unblock for you.** It gets a GL-capable device to
a working scanout, which is the floor under anything Mesa-shaped. It does not
give you a rendering context: the driver creates one resource with the 3D
command and never issues `CTX_CREATE` or `SUBMIT_3D`, so there is no command
stream to virglrenderer yet. Whether that gap belongs to the kernel driver or
to a userspace DRM/Mesa path is a Lane C call, and if it turns out to want
kernel work, file it back.

**Q51 / §59 are untouched by this note.** §59 is `Decided by: Operator` and
stays as it is; this changes one premise it rested on (the environment now
exists), which is exactly the fact the standing audit wanted surfaced — it is
not a re-decision, and Lane A has not treated it as one.
