## §268 — A virtio-gpu render resource is not a GEM object, because the two disagree about how wide a row is

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The graphics card can hold images for a program to draw into.
There were already two ways to ask for one — the old "dumb buffer" path and the
newer "render resource" path that lane C's compositor wants — and it was
tempting to make the second reuse the first's machinery, since that would have
supplied memory allocation, lifetime and `mmap` for free. It cannot: the two
disagree about how many bytes a row of pixels occupies, and the disagreement is
silent. An image whose width is not a multiple of 16 pixels would come out
skewed — each row shifted a little further sideways than the last — with no
error anywhere. So render resources get their own allocation and their own
table, and only the *`mmap` token space* is shared.

### The concrete conflict

`PixelFormat::pitch(width)` — the dumb-buffer/GEM sizing function — pads each
row out to a 64-byte boundary, which is standard practice and what the display
hardware wants. So `Xrgb8888.pitch(100)` is **448**, not 400.

`VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D`, the command that ships a render
resource's pixels to the host, **carries no stride field at all**. The host
computes the row start itself, as `width * bytes_per_pixel`. There is nowhere to
tell it about padding.

A GEM-backed render resource would therefore hand the host a buffer laid out at
448 bytes per row while the host read it at 400. Every row after the first would
be misaligned by a growing multiple of 48 bytes. Nothing would report an error:
the transfer succeeds, the sizes are plausible, the picture is wrong. Widths
that *are* a multiple of 16 (which includes every screen resolution anyone would
test with first) work perfectly, so this would have survived a long time.

### What was given up

Real duplication. `GemObject` already has: frame allocation and freeing,
per-object refcounting, a handle table, `mmap` offset issuance, and teardown on
process exit. The resource manager reimplements the first four. That is roughly
150 lines that exist twice, and a second lifetime scheme to keep correct.

The alternative — teach `TRANSFER_TO_HOST_2D` to un-pad, by issuing one transfer
per row — was considered and rejected on cost: a 1080p resource would become
1080 separate device commands, each with its own descriptor and response, on
what is meant to be the fast path.

### What is shared, and why that part is safe

The **`mmap` fake-offset space** is shared, via an explicit `Mappable` enum with
a `Gem` and a `VirtgpuResource` variant. The tempting alternative was a second
parallel `offset_for_virtgpu`/`lookup_virtgpu` pair, which would have been a
smaller diff and touched no existing call site. Rejected: `mmap`'s offset
argument is one number space as far as userspace is concerned, and two
allocators handing out numbers from it independently will eventually hand out
the same one — at which point an `mmap` returns *somebody else's buffer*. The
enum makes that unrepresentable, and `dumb_mmap`'s self-test now asserts that a
GEM handle and a resource id with the same numeric value do not alias.

### If this turns out wrong

The signal would be a second consumer wanting a render resource that *is* also a
scanout buffer (a compositor mapping one image both as a render target and as
something the display controller reads). That needs one object with two layouts,
which neither design gives you. The fix then is not to merge the two paths but
to add an explicit re-layout step between them — which is cheap to add later and
would have been invisible if the layouts had been silently conflated now.
