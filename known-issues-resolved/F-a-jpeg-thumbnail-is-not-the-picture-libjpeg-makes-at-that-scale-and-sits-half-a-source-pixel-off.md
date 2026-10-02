### [F] A JPEG thumbnail is not the picture libjpeg makes at that scale, and sits half a source pixel off -- 2026-09-25 -- **FIXED 2026-09-25**

**Status:** ✅ FIXED 2026-09-25 (lane F) — design-decisions §1307. The reduced
transforms now average as libjpeg-turbo's do, and each component is
reconstructed at the block size libjpeg gives it; every fixture at 1/2, 1/4
and 1/8 is within 2 levels of TurboJPEG's own scaled decode
(`tests/jpeg_sampling.rs`), where it was up to 157. The entry below is kept as
the record of what was wrong.

**In short:** a JPEG thumbnail is made by decoding the photograph directly at a
half, a quarter or an eighth of its size, which is far cheaper than decoding it
whole and shrinking it. This decoder does that reduction its own way: at half
and quarter size its picture is shifted by half an original pixel, and it is not
the picture libjpeg (under every browser and image library) makes at the same
size, so there is nothing to check it against. Nobody would see the shift in a
thumbnail; having no reference to test the scaled path against is the real
cost.

**Where.** `gui/imagecodec/src/jpeg.rs`: `idct_scaled` and `scaled_position`,
and every component sharing one block size in `decode_scan` and in
`jpeg/progressive.rs`'s `Coefficients`.

**What differs.**
1. `idct_scaled` transforms only a block's top-left `n` x `n` coefficients and
   evaluates the 8-point basis at whole positions (`scaled_position`: 1, 3, 5
   and 7 at half scale). The centres of the pixel pairs those samples stand for
   are at 0.5, 2.5, 4.5 and 6.5 — half a source pixel away, the offset
   `scaled_position`'s doc comment says it avoids.
2. libjpeg-turbo's reduced transforms (`jidctred.c`: `jpeg_idct_4x4`, `2x2`,
   `1x1`) produce, by their own header comment, the mean of each 2x2 (4x4, 8x8)
   group of the full transform's output: a box filter done inside the
   transform, which needs every coefficient that contributes to those means
   (all but row and column 4 at half scale; 0, 1, 3, 5 and 7 at quarter).
3. For colour halved both ways (4:2:0) libjpeg-turbo reconstructs chroma with a
   transform twice the size instead of upsampling it afterwards (`jdmaster.c`:
   "scale up the chroma components via IDCT scaling rather than upsampling").

**The proper fix:** what libjpeg-turbo does — the averaging reduced transforms,
and the doubled chroma transform wherever both ratios allow it — so the scaled
path can be held to TurboJPEG's own scaled decode (`simplejpeg`, already what
writes two of the fixtures). The cost to weigh: a progressive thumbnail must
then keep 7x7 coefficients a block at half scale and 5x5 at quarter instead of
4x4 and 2x2 (design-decisions §1305, point 1); eighth scale keeps the DC alone
either way.

**How to see it.** Decode a JPEG with `decode_scaled` at a bound that picks half
scale, and compare it with `simplejpeg.decode_jpeg(data, min_factor=2)`.
