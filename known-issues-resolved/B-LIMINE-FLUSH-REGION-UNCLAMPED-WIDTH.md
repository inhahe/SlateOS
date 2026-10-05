## B-LIMINE-FLUSH-REGION-UNCLAMPED-WIDTH (lane A, 2026-08-22) — FIXED 2026-08-22

**What it was:** `LimineBackend::flush_region` in `kernel/src/drm/driver.rs`
clamped the rectangle's `x`, `y` and `h` against both the source framebuffer
and the display, and did not clamp `w` at all. `copy_w_bytes` came straight
from the caller as `w * bpp`. A client that asked to flush a rectangle wider
than the display therefore wrote past the right edge of every row — harmlessly
into the next row for rows above the last, and past the end of the firmware
framebuffer entirely on the last one.

Found while auditing all three DRM backends for the bug class behind
`B-VIRTIO-GPU-FLAT-SCANOUT-WILD-WRITE`. It is a different bug with the same
shape: an unbounded destination write derived from caller-supplied geometry.

**Why it was never observed:** the compositor is the only in-tree caller and it
derives damage rectangles from the surface it is drawing, so `w` was always
within the display. The bug needed a client that got its own geometry wrong —
which is precisely the client a kernel must not trust.

**Second, quieter bug in the same two functions:** both Limine paths advanced
into the GEM object's frame list once per row and copied
`min(row_bytes, FRAME_SIZE - frame_offset)`, i.e. they handled at most one
source frame crossing per row. A row wider than one 16 KiB frame — 4096 px at
32bpp — crosses two or more, and every crossing past the first was dropped.
Not a memory-safety bug (it copies less, never more), but at 5K and above the
right of every row would have stayed stale, which is the kind of artifact that
gets blamed on the compositor.

**Fix:** `blit_run_flat`, the flat-destination counterpart to `blit_run`. It
walks the source frame list generally, once per boundary, and truncates the run
against `dst_len` before copying anything. Both Limine paths use it;
`flush_region` additionally clamps `w` against
`min(fb.width, self.width) - x_start`. Commit `b050e0bd5`.

**Why it discards rather than errors:** same reasoning as
`design-decisions.md` §271 (*non-contiguous framebuffer*) — an over-wide
rectangle now yields a visibly
clipped picture, which gets reported as a display bug, rather than corruption
in whatever the firmware mapped after the framebuffer, which gets blamed on a
subsystem three layers away.

**Backend audit result, for the record:**

| Backend | Destination | Verdict |
|---|---|---|
| virtio-gpu | 250 discontiguous 16 KiB frames | was wrong on both sides — `B-VIRTIO-GPU-FLAT-SCANOUT-WILD-WRITE` |
| Limine | genuinely linear firmware aperture | destination safe; unclamped width + dropped source crossings — this entry |
| ATI | zero-copy VRAM, no blit at all | clean |
