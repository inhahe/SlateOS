## 1500. How §976 is built: a change is validated as the state it leaves, and a cursor the device cannot draw is composed

**Date:** 2026-09-27 · **Decided by:** Claude (operator-approved scope: the operator decided in §976 that plane rectangles are honoured and virtio-gpu's cursor goes on its cursor queue; the choices below are how) · **Lane:** A

**In short:** when a program asks the display to show something, the kernel
now works out the whole picture that would result, checks that picture, and
only then changes the screen. If any part cannot be shown exactly as asked,
nothing changes and the program gets an error saying whether its request was
malformed or just not possible on this display. The mouse pointer is drawn by
the virtual display device itself when it fits the device's 64x64 pointer, and
by the kernel otherwise, so every pointer is shown.

**1. Validate the resulting state, not the request.** `set_crtc`, `page_flip`
and an atomic commit take a snapshot of the object model, record the change,
validate every scene it changes (`DrmDevice::validate_scene`), and only then
program or draw; any refusal puts the snapshot back. `atomic_check` is the same
code with the snapshot always put back.
- *Rejected: check the request, then apply it.* That is two implementations of
  what a commit does -- one to check, one to apply -- and they drift. The
  concrete case: a commit that sets a mode re-fits the primary plane to it,
  and a request-level check would judge the plane by its old rectangles.
- *Cost:* a snapshot clones the CRTC and plane lists (a handful of small
  structs) per mode-set, flip or commit. Not on the cursor path.

**2. Two error codes, in a fixed order.** A malformed layer -- source outside
its buffer, scale beyond 16x, a format its plane does not list -- is
`InvalidArgument` on every backend, and is checked first. A well-formed scene
this backend cannot show -- anything composed, on the ATI backend -- is
`NotSupported`. A client can tell "never valid" from "not here", and a buffer
too small for the mode stays `InvalidArgument` everywhere, as before.
- *Rejected: per-backend scale limits* (`Limits::UNSCALED` on ATI). A scaled
  plane on ATI would then be "malformed" there and fine elsewhere.

**3. A plane's format list is enforced.** Every primary plane lists XRGB8888
and ARGB8888, and nothing checked it: an RGB565 or BGR buffer was byte-copied
into the scanout. It is `InvalidArgument` now, as in Linux, rather than
converted. Converting would be a second pixel path to keep correct, for
formats no client here uses.

**4. `flush_region` redraws only where its buffer is shown**, mapped through
each plane that shows it (`planecompose::source_to_dest`), as Linux's
`DIRTYFB`. A buffer nobody shows is a successful no-op. Before, a flush copied
the region onto the scanout whatever was being shown.

**5. The hardware cursor.** virtio-gpu draws the legacy cursor itself
(`UPDATE_CURSOR`/`MOVE_CURSOR` on queue 1) when the image is at most 64x64,
its hot spot is inside it, and the hot spot's position is not negative; the
device shows it only while the DRM owns the screen (a mode and a primary
framebuffer), as a composed cursor is only drawn then. A cursor the device
cannot take is composed, so every cursor is shown.
- *Rejected: refuse a cursor the device cannot take.* §976 asks for
  requests to be honoured, and composition honours it.
- One sync, `DrmDevice::sync_hw_cursor`, compares what the device is showing
  with what the model says and sends only the difference. It runs after every
  change that can affect it: the cursor calls, a mode-set, a flip, a commit, a
  framebuffer's destruction. The alternative -- each path sending its own
  commands -- is how a cursor gets left on a screen that was turned off.
- The image goes to the device as the client wrote it, premultiplied, as
  Linux's virtio-gpu driver sends it. QEMU's display back-ends draw it
  unpremultiplied, so a half-transparent edge pixel comes out slightly dark.
  Only anti-aliased edges are affected.
- The move is synchronous: each `MOVE_CURSOR` waits for the device to consume
  it, like every other command in this driver. A queue of in-flight moves
  would save that wait at the cost of a second command path; it is the thing
  to build if pointer latency is ever measured to matter.

**6. On a device failure after validation**, the snapshot is restored and the
touched CRTCs are redrawn from it (`DrmDevice::reshow`), and the device's
cursor re-synced, so the screen agrees with the model again. It is not a second
mode-set: a backend that has just failed to program one is not asked to program
another from inside its own error path.
