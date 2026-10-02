## §271 — A framebuffer that is not contiguous does not get to expose a base pointer

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** the virtio-gpu screen buffer is stored as 250 separate chunks of
memory, not one block, but the driver offered callers a single "here is the
framebuffer address" function. Two of its three callers then did the obvious
thing — address + row × width — and wrote four megabytes of pixels over
unrelated kernel memory. The choice was between fixing those two callers and
removing the function that made the mistake natural. The function was removed.

### What was on the table

**Option A — fix the two blit sites, keep `framebuffer_addr()`.** Smallest
change. Rejected: the accessor's contract is "a pointer to the framebuffer",
its actual meaning is "a pointer to 1/250th of the framebuffer", and nothing in
its type expresses the difference. Two of the three call sites in the tree had
already got it wrong; there is no reason to expect the fourth to do better.
Fixing the callers leaves the trap armed.

**Option B — make the scanout physically contiguous.** Then the base pointer
would be honest and every caller's arithmetic would be correct as written.
Rejected on two grounds. It asks the buddy allocator for a 4 MiB contiguous
run at probe time, which is a request that can fail after uptime and would make
display init fragmentation-sensitive for no user-visible gain; and at 4K it
becomes a 32 MiB contiguous request, which is worse. Meanwhile the device does
not need contiguity — `attach_backing` already hands the host a scatter list —
so this would be paying a real allocation constraint purely to make one
pointer's arithmetic work out.

**Option C (taken) — replace the accessor with a bounds-checked view.**
`with_scanout(|sc| …)` lends a `ScanoutMem` whose `write_at` walks the frame
list and clamps to the end of the buffer. The frame layout stops being
something every caller must know and re-derive; it is stated once, in the
module that owns it.

### Why the clamp discards rather than errors

`write_at` returns the number of bytes it wrote and silently drops anything
past the end, instead of returning `Err`. A blit is a per-row inner loop and
threading a `Result` out of it would either abort a half-drawn frame partway or
be ignored at the call site — and an ignored error is the state we just came
from. Discarding converts a class of caller arithmetic bugs from *kernel
memory corruption* into *a visibly wrong picture*, which is the failure mode
that gets reported and fixed rather than blamed on something else three
subsystems away. The byte count is there so a test can assert the strong
property, and `virtio::gpu::self_test` does: a write at `sc.len()` must return
`0`, and a write at the last pixel's offset must return `4` and be readable
back through an independent frame walk.

### The cost that was accepted

`with_scanout` holds the device spin lock for the whole blit, where the old
code took it only to read the base address. This is a widening of the hold
time, and it was accepted rather than worked around: the old release-then-write
pattern meant a concurrent mode-set replacing `fb_frames` would leave the blit
writing into freed frames, which is the same bug class again. Nothing in an
interrupt or panic path touches this device, and `flush_full` already holds the
same lock across a virtqueue round-trip to the host — longer than the memcpy
this now covers — so no window was widened past one that already existed.

Full diagnosis, including how the bug was localised and its likely identity
with B-KNULLJUMP: `known-issues.md` →
`B-VIRTIO-GPU-FLAT-SCANOUT-WILD-WRITE`.
