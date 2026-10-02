## §264 — Mesa is on the roadmap, sequenced by what it unblocks rather than by 3D itself

**Date:** 2026-08-21
**Decided by:** Operator (Claude recommended treating this as a separate later
call; operator decided it now)
**Lane:** A

**In short:** Nothing in this OS has ever drawn a 3D image. The missing piece is
Mesa, the library that turns 3D drawing commands into work for a graphics chip.
The decision is that we *will* port it — it is no longer parked or
"indefinitely deferred" — but it does not jump the queue ahead of wifi, a web
browser, and the rest. The operator asked a sharp follow-up: does a browser need
Mesa? The answer is that it uses it heavily but does not require it, which is
what sets the ordering.

### The question that decided the ordering

> "will chromium use mesa for webgl/webgpu support? in that case, maybe do mesa
> before chromium?"

Chromium's entire GPU stack routes through Mesa on Linux: WebGL goes through
ANGLE onto GLES/EGL or ANGLE's Vulkan backend; WebGPU goes through Dawn onto
Vulkan; and separately from either, GPU compositing, GPU rasterization and
VA-API video decode all do too. So Mesa is not a WebGL side-feature — it is most
of what makes a browser feel like a browser.

**But Chromium bundles SwiftShader**, a software renderer, and falls back to it
when no GPU driver is present. So Chromium without Mesa *runs*, including WebGL;
it is slow, it burns CPU, and WebGPU is far more likely to be disabled outright.
That makes Mesa a **performance** prerequisite for Chromium, not a functional
one — which is exactly the distinction that lets it be scheduled rather than
blocking.

Conclusion, and it agrees with the operator's instinct: **Mesa before Chromium**,
because a browser is the single largest consumer of it and shipping one that
software-renders every frame is a bad first impression that we would then have
to re-earn. But **wifi before Mesa** — a machine that cannot get on a network
has nothing for the browser to display.

### What is already done, and what "3D works" would still require

Option A of the original question is **built and tested** (§243): the 3D-capable
emulated device boots green, full display bring-up, and the `SET_SCANOUT:
resp=0x1203` regression is fixed. `SLATE_GPU=virtio-gpu-gl-pci ./scripts/boot-test.sh`
is the flag.

The caveat must be restated because A's completion makes it easy to over-read:
**nothing renders 3D.** The driver issues exactly one command from the 3D set —
the one that allocates the framebuffer — and never creates a rendering context
or submits a command stream. `3D_FEATURES = 0` and the empty capset list in
`kernel/src/drm/virtgpu_uapi.rs` are still correct and still untouched. "The 3D
device boots" is not "3D works".

### Lane note

Mesa lands in `gui/**`, which is **lane C's** zone; it competes with the
compositor and desktop queue, not with kernel work. Lane A's contribution is
done. §59's stale premise — that no 3D test environment exists — is superseded
by this entry.
