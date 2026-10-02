### [F] Lossless WebP decodes at half libwebp's speed -- 2026-09-25 -- **FIXED 2026-09-25**

**Status:** FIXED — it now takes fewer cycles than libwebp: 0.39 billion for
the 2000x1500 picture against libwebp's 0.46 (it was 0.63), measured in the
decoding thread's CPU cycles. The pixel loop looks its prefix-code group up
once a block and walks columns without dividing (libwebp's own scheme, item
1 below), non-overlapping copies are one move, and the predictor and colour
transforms work a row at a time in runs of one block, their mode or element
looked up once a run (in the spirit of item 3). Every fixture as before,
and 3,750 bit-flipped and 2,183 truncated lossless and alpha WebPs decoded
exactly as libwebp decodes them. Item 2 was not needed. The original report
follows.

**In short:** opening a large lossless WebP takes about twice as long here as
in a browser: 0.31 s for a 2000x1500 picture against libwebp's 0.15 s. The
pictures are right to the bit; only the speed is behind. Lossless WebP is rare
for photographs (those are almost always lossy), so this is felt mainly on big
lossless screenshots and artwork.

**Where.** `gui/imagecodec/src/webp/lossless.rs`, `decode_image`'s pixel loop.

**The proper fix,** each measurable on its own against
`examples/time_decode.rs`:
1. Look the prefix-code group up once per block of `2^prefix_bits` pixels
   rather than per pixel, as libwebp does.
2. When a group's red, blue and alpha codes are all single-symbol or short,
   decode a literal's four channels from one table lookup (libwebp's "packed"
   tables).
3. Undo the transforms a row at a time into one buffer instead of one pass per
   transform over the whole picture, so the image stays in cache.

**How to see it.** `cargo run --release -p imagecodec --example time_decode --
<lossless.webp>`, against Pillow's `Image.open(...).load()` on the same file.
