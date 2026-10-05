## 59. GPU acceleration scope (Q18) — build the kernel-side virtio-gpu render-ioctl dispatch now with honest "no-3D" reporting; defer the Mesa port (option B)

**Date:** 2026-07-14
**Decided by:** Operator (Claude recommended C; operator chose B).

**Context.** Q15 green-lit the GPU-accel initiative; the 2D foundation is done.
Real 3D is gated on a virgl-capable test environment (our headless TCG CI exposes
plain virtio-gpu with **no** `VIRTIO_GPU_F_VIRGL`) and the large external Mesa
port.

**Decision.** Build the kernel-side virtio-gpu render-ioctl dispatch now with
**honest "unsupported" reporting**: `GETPARAM` reports `3D_FEATURES=0`,
`GET_CAPS` returns no capsets, every 3D-requiring ioctl returns the correct
errno; verified by a ring-3 self-test that opens `renderD128` and issues the
ioctls. The Mesa port stays deferred until a virgl test environment exists.

**Alternatives.** (A) Invest in virgl env + Mesa now — deferred. (C) Stop at the
foundation — operator chose to land the ioctl ABI now.

**Where it lives.** `kernel/src/syscall/linux.rs` `drm_card_ioctl` (new
`DRM_COMMAND_BASE`-range arm routing to `drm::virtgpu_uapi`), plus a ring-3
`renderD128` ioctl self-test.

**2026-08-18 — the deferral's prerequisite has fired, and the flip is not free.
Measured, not inferred; the decision is NOT being revisited here.** This entry
defers the Mesa port "until a virgl test environment exists", which makes that
clause exactly the kind of premise the S305 standing audit exists to re-check.
It no longer holds.

Two configurations of the *same* kernel image, changing only the GPU device and
the display backend, with `kernel/src/virtio/modern.rs::negotiate` reporting the
mask the **device offered** (it logs that independently of what we accept):

| QEMU flags | Features offered (page 1 : page 0) | Page-0 bits |
|---|---|---|
| `-device virtio-gpu-pci -display none` | `0x00000101 : 0x30000002` | 1 (EDID) |
| `-device virtio-gpu-gl-pci -display egl-headless` | `0x00000101 : 0x30000013` | **0 (VIRGL)**, 1 (EDID), 4 (CONTEXT_INIT) |

The first row is the number this entry already quotes (`0x30000002` — EDID
only), which is what makes the pair a controlled comparison rather than two
unrelated readings. The second has **bit 0 set: `VIRTIO_GPU_F_VIRGL`, offered to
our own driver, in a headless run on this machine.** QEMU 11.0.93; the enabling
detail is that `-display none` has no GL at all ("The display backend does not
have OpenGL support enabled", exit 1) while **`egl-headless` provides GL without
a window** — so the environment was never missing, only unreachable through the
display backend the harness happens to pass.

**But the same run shows that adopting it today would regress the 2D path.**
With the GL device our virtio-gpu init fails partway:

```
[virtio-gpu] Attached backing memory
[virtio-gpu] SET_SCANOUT: resp=0x1203        # ERR_INVALID_RESOURCE_ID
[virtio-gpu] Init: IoError (non-fatal)
```

and QEMU prints `virtio_gpu_virgl_process_cmd: ctrl 0x103, error 0x1203`. The
consequence is visible one subsystem up — **the DRM device count drops from 2 to
1 and virtio-gpu stops being the primary display**:

| | `[drm]` outcome |
|---|---|
| plain | `Registered device 1 (virtio-gpu, …)` → `virtio-gpu set as primary display` → `2 devices` |
| GL | *(no virtio-gpu registration)* → `1 device` |

Cause: QEMU's `virtio-gpu-gl` routes commands through virglrenderer, which does
not honour a plain `RESOURCE_CREATE_2D` resource as a scanout target — and it
does so **even though we accepted no page-0 features** (`accepting
0x00000001:0x00000000` in both rows). So this is not a flag flip; the 2D scanout
path needs work before the harness could adopt the GL device.

**What this does and does not establish.** It establishes that a virgl-capable
*headless* environment exists and that our driver is offered the bit — the exact
premise the deferral rests on. It does **not** establish that a 3D stack works:
nothing here negotiates VIRGL, creates a 3D context, or renders. `param_value()`
in `kernel/src/drm/virtgpu_uapi.rs:503` therefore stays honest (`3D_FEATURES =
0`, `NUM_CAPSETS = 0`) — its own doc comment already names this measurement as
the trigger for flipping, and flipping it before a backend exists is precisely
the dishonest reporting option B was chosen to avoid.

**Not resolved here, deliberately.** This is an **Operator** decision, so the
premise change is recorded and routed rather than acted on: see
`open-questions.md` **Q51**. The Mesa port itself is lane C's zone (`gui/**`),
notified in `requests/a-c-the-virgl-test-environment-that-blocked-mesa-now-exists.md`;
the kernel-side virtio-gpu/DRM half above is lane A's.

Reproduce (~90 s, needs the boot lock; the harness is unmodified):

```bash
qemu-system-x86_64 \
  -drive if=pflash,format=raw,readonly=on,file=<ovmf>/edk2-x86_64-code.fd \
  -drive format=raw,file=fat:rw:<repo>/build/esp \
  -device virtio-gpu-gl-pci -vga std -display egl-headless \
  -serial file:<out>.txt -m 3072M -machine q35
grep -a 'Features offered' <out>.txt
```

**ANNOTATION 2026-08-19 (lane A) — the 2D regression above is fixed; the
deferral itself is untouched.** The `SET_SCANOUT: resp=0x1203` failure and the
"2 devices → 1, no primary display" consequence quoted above no longer occur:
`SLATE_GPU=virtio-gpu-gl-pci ./scripts/boot-test.sh` boots the GL device green,
with virtio-gpu registered and primary, and the default device unchanged and
separately re-tested. The reproduction snippet above is superseded by that flag.

The cause named above — "virglrenderer does not honour a plain
`RESOURCE_CREATE_2D` resource as a scanout target" — is correct but understates
why: QEMU's translation of that command hardcodes `bind = RENDER_TARGET` and the
command has no `bind` field, so *no guest* can produce a scanout-capable
resource with it; virglrenderer allocates the shared D3D11 texture that
`SET_SCANOUT` needs on Windows only when `bind` includes `SCANOUT`. The driver
now uses `RESOURCE_CREATE_3D` with that bind on a device offering
`VIRTIO_GPU_F_VIRGL`, and accepts that bit for that purpose alone. See §243.

**This does not flip this entry.** §59 is an Operator decision and stays as
written. What changed is one supporting fact — the sentence "the 2D scanout path
needs work before the harness could adopt the GL device" has been satisfied. The
paragraph beginning "What this does and does not establish" is unchanged and
still governs: nothing negotiates a 3D context or renders, and
`param_value()`'s honest zeros are untouched. `open-questions.md` Q51 carries
the same update and is now a choice between its options B and C only.
