## §243 — On a virgl-capable virtio-GPU the framebuffer is created with the 3D command, and `VIRTIO_GPU_F_VIRGL` is accepted purely to be allowed to do so

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short.** QEMU has two virtio display devices: the plain one
(`virtio-gpu-pci`) and the GL-accelerated one (`virtio-gpu-gl-pci`). We booted
fine on the plain one and showed a black screen on the GL one — the last step
of display setup was rejected, so nothing was ever put on screen. The cause is
that on the GL device the host will only let you *show* a picture whose memory
was requested with a flag saying "this is going to be shown", and the simple
"make me a 2D image" command we were using has no field to say that. The fix is
to use the richer 3D-flavoured create command on that device only, and to
answer "yes" when the device asks whether we speak the 3D protocol — not
because we intend to render 3D, but because saying "no" does not get us off the
3D path, it only forbids us from using the one command that works on it.

**The chain, in one paragraph.** As soon as the device is `virtio-gpu-gl`, QEMU
routes *every* command through virglrenderer — declining the feature bit
changes nothing about the routing. QEMU's translation of the 2D create
hardcodes `bind = RENDER_TARGET`, and the 2D command has no `bind` field, so no
guest can influence it. virglrenderer allocates the shared D3D11 texture only
when `bind` includes `VIRGL_RES_BIND_SCANOUT` (1 << 18). Without that texture,
the call QEMU makes on `SET_SCANOUT` returns `EINVAL` on Windows, and QEMU
reports `ERR_INVALID_RESOURCE_ID` (0x1203). Full evidence, including the
debugger session that established it, is in `known-issues.md` under the
2026-08-19 RESOLVED entry.

**The alternatives, and why not.**

*Leave the GL device unsupported and keep negotiating zero features.* This was
the status quo, and it is what made the entry defensible for a while: the plain
device works, and the GL device is not the default. It is rejected because the
GL device is not an exotic configuration — it is the **only** QEMU display that
can host Mesa, so leaving it broken leaves Lane C's entire GL story untestable,
and the failure it produces is a black screen with a numeric error, which is
exactly the shape of bug that costs someone a day when they meet it cold.

*Always create with `RESOURCE_CREATE_3D`, on every device.* Simpler code — one
path instead of two — and it would work today, because QEMU's plain 2D device
also accepts the 3D create. It is rejected because it makes correctness on the
common device depend on an accident of QEMU's implementation: the 3D command
block is specified as belonging to a device that offered `VIRTIO_GPU_F_VIRGL`,
and sending it to a device that did not offer it is a protocol violation that
happens to be tolerated. Real hardware and other hosts (and future QEMU) are
entitled to reject it, and the resulting failure would be a black screen on the
*default* device — trading a rare breakage for a universal one.

*Try 3D first and fall back to 2D on failure, unconditionally.* This is what
the code does *within* the virgl branch, but not across it. Making it the sole
strategy would mean every plain-device boot sends a command it expects to fail,
which costs a virtqueue round trip and, worse, teaches the log to contain an
error line on a healthy boot. A driver whose normal output includes a failure
message is a driver whose real failures do not get noticed.

**Why accepting the feature bit is not the same as claiming 3D support.**
`VIRTIO_GPU_F_VIRGL` in the spec means "the driver understands the 3D command
set". We use exactly one command from that set and never create a rendering
context, so the claim is technically broader than the use. The narrower reading
— accept nothing, use nothing — is unavailable, because QEMU checks the
negotiated bit before dispatching `RESOURCE_CREATE_3D` while *not* checking it
before routing through virglrenderer. There is no state in which we are both
off the virgl path and permitted to ask for a scanout-capable resource. Given
that, accepting the bit is the honest description of what we do: we do speak
enough of the 3D protocol to allocate our framebuffer with it.

**The fallback is retained and is not dead code.** If `RESOURCE_CREATE_3D`
fails on a virgl device we log and fall back to the 2D create, which then
almost certainly fails at `SET_SCANOUT`. That is deliberate: the alternative is
to abort display initialisation entirely, which turns a possibly-recoverable
host quirk into no console at all. The log line says the fallback happened and
that scanout is unlikely to work, so the failure is attributable rather than
mysterious.

**What would change this decision.** A host that offers `VIRTIO_GPU_F_VIRGL`
but rejects `RESOURCE_CREATE_3D` for a plain framebuffer (then the fallback
stops being a safety net and becomes the main path, and the shape of the
negotiation needs rethinking); or virglrenderer gaining a way to promote an
existing resource to scanout-capable after creation (then the 2D create could
stay, with a follow-up command).

**Harness consequence.** None of this was reachable by the test suite before,
because `scripts/boot-test.sh` hardcoded `-device virtio-gpu-pci -display none`
and `QEMU_EXTRA` can only *add* a second GPU, which the driver would not bind.
`SLATE_GPU=<device>` now selects the device under test and switches the display
backend to `egl-headless` (QEMU refuses to host a GL device on `-display
none`), and marks the run an experiment so its wall-clock cannot pollute the
default-configuration population — the same treatment `QEMU_EXTRA` and the
accelerator override already get.
