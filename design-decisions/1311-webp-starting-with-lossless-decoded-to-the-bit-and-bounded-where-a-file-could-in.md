## 1311. WebP, starting with lossless: decoded to the bit, and bounded where a file could inflate it

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** WebP is now the web's commonest picture format, and nothing here
read it: a downloaded WebP showed as a plain coloured rectangle in the file
manager and would not open. This adds the container and the lossless half of
the format, which decodes to exactly the pixels libwebp (the reference, under
every browser) produces. The lossy half -- a VP8 video frame, which is most
WebP photographs -- and animation are refused by name for now and come next.

### What it is

`gui/imagecodec/src/webp.rs` (the RIFF container, the extended `VP8X` header)
and `webp/lossless.rs` (RFC 9649 §3): prefix codes built as two-level lookup
tables; pixels that are literals, LZ77 copies (with the two-dimensional
distance map) or colour-cache hits; per-block prefix-code groups chosen by an
entropy image; and the four inverse transforms -- predictor (all fourteen
modes and the edge rules), colour, subtract-green, and colour indexing with
pixel bundling.

### The choices with two sides

1. **Lossless first.** Lossy WebP is the more common file, and a larger decoder
   (RFC 6386's VP8 key frame). *For lossless first:* it is self-contained, its
   container code is the same, and being exact by definition it can be proven
   against libwebp to the bit before the lossy decoder -- whose alpha plane is
   itself lossless-coded -- is built on it. *Against:* most WebP photographs
   still do not open until lossy lands; `decode` says so by name
   (`ImageError::Unsupported("lossy WebP")`) rather than failing obscurely.
2. **Only the prefix-code groups a block uses are kept.** The entropy image may
   name up to 65,536 groups of five codes, all of which are in the stream and
   must be parsed to reach the pixels; a few hundred kilobytes of file could
   otherwise make the decoder build gigabytes of tables. Each group is parsed and
   validated; only the used ones are built and kept, and their tables count
   against the caller's byte budget (`Limits::max_decompressed_bytes`), as the
   pixels do. libwebp does the same.
3. **A prefix code must describe a complete tree**, one symbol excepted, as
   libwebp requires. The RFC says the tree "must be" complete but not what to do
   when it is not; following the reference means a file libwebp refuses is
   refused here too, rather than decoded to something no one else shows.
4. **A cut-off file is an error, not a partial picture.** Unlike progressive
   JPEG or interlaced GIF, a lossless stream's early part is not a coarser
   picture -- the transforms can only be undone once all the pixels are in -- so
   there is nothing honest to show. `ImageError::Truncated`.

### How it is held

`tests/webp.rs` over twelve libwebp-written fixtures, every pixel of every one
compared exactly, transparent pixels' colours included: a photograph at the
most and the least encoder effort, a larger one, one with every kind of alpha,
two, four, thirteen and a hundred colours (colour indexing at each of its four
bundling widths), one pixel, one row, one column, and the extended container. A
unit test in `webp/lossless.rs` checks that between them the fixtures still use
every tool the format has, so a regenerated set cannot quietly test less. Unit
tests pin the prefix-code construction (canonical order, second-level tables,
the one-symbol case, incomplete and over-subscribed trees refused), the distance
map, the channel arithmetic, `Select`'s tie-break, the colour transform's order
and the predictor's edge rules. Every prefix of the small fixtures and a spread
of bit flips decode without a panic.

### Measured

Release, a 2000x1500 lossless photograph (3.7 MB): 0.31 s, where libwebp (through
Pillow) takes 0.15 s. Twice as slow as a hand-tuned C library with vector
paths; faster decoding (literals decoded several channels at a time, the group
looked up once per block rather than per pixel) is recorded in `known-issues.md`.
