## TD-COMPOSITOR-COPIES-EVERY-FRAME-TWICE (lane C, 2026-08-21)
**Status:** OPEN — 2026-09-24 (lane F): the per-frame copy is unchanged, but the compositor no longer keeps a pair of its own — it composites into one buffer (design-decisions §1300), a frame of memory less — and `RenderTarget::buffer_age` plus the damage history now make a multi-buffered target correct, which is the prerequisite for composing straight into the scanout pair. Still blocked on `TD-NO-WRITE-COMBINING`, as below.

**In short:** every frame the desktop draws is copied one extra time on its way
to the screen. It is correct, just wasteful — and the waste grows with screen
size, so it matters most on the big displays where it is least affordable.

**What.** `Compositor::compose_frame` blends into its own back buffer, and then
`DrmScanout::show` copies that whole buffer into the mapped scanout buffer,
row by row at the driver's pitch. At 4K that is ~33 MB moved per frame, ~2 GB/s
at 60 Hz, for a copy that produces no pixels.

**The proper fix.** Compose *directly into* the mapped scanout buffer. The
compositor already owns a back/front pair, and so does the scanout — they are
the same pair, duplicated. The obstacle is that `Compositor` allocates its
buffers as `Vec<u32>` at construction and hands out `&[u32]`, whereas the
scanout's are kernel-mapped `&mut [u8]` at a pitch that is not `width * 4`.
Reconciling them means `Compositor` composing into a caller-supplied buffer with
a caller-supplied stride, which is a real refactor of the rasteriser's
addressing and touches every draw path.

**Also blocked on `TD-NO-WRITE-COMBINING`.** Composing into the mapped buffer is
only a win if that memory is write-combining. Blending *reads* the destination,
and reads from uncached video memory are catastrophically slow — an order of
magnitude worse than the copy this replaces. Do not attempt this optimisation
until write-combining is confirmed on the mapping.

**Severity.** Low now — nothing is measured yet and QEMU is not where this is
judged. Revisit when there is a frame-time budget to hold, and measure before
touching the rasteriser.
