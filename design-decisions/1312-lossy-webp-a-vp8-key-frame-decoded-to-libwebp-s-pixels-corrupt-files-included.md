## 1312. Lossy WebP: a VP8 key frame decoded to libwebp's pixels, corrupt files included

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** Most WebP pictures are lossy -- photographs saved by browsers,
phones and every image tool -- and until now they did not open here. They do
now, with their transparency, and every pixel is the one libwebp (the decoder
inside Chrome, Firefox and Android) produces: the same colours to the last
bit, over 36 test pictures that between them use every feature the format
has. Where the format's own reference decoder and libwebp disagree -- only on
files no encoder writes -- this follows libwebp, so a damaged or unusual file
looks here as it looks in a browser.

### What it is

`gui/imagecodec/src/webp/lossy.rs` and `webp/lossy/`: the frame header and
per-macroblock modes, the coefficient tokens (`lossy.rs`), the boolean
entropy decoder (`reader.rs`), intra prediction (`predict.rs`), the inverse
DCT and Walsh-Hadamard transforms (`transform.rs`), the loop filter
(`filter.rs`), and the conversion to RGB (`yuv.rs`); VP8's probability and
quantiser tables (`tables.rs`) are generated from RFC 6386's text, whose two
copies of each were checked against each other number by number.
`webp/alpha.rs` decodes the `ALPH` chunk: raw or lossless-compressed, under
any of the four predictive filters.

### The choices with two sides

1. **libwebp, not the RFC's reference decoder, where they part.** RFC 6386
   ships a reference decoder ("dixie") and libwebp departs from it in a few
   places no encoder reaches: a segmentation with no values of its own is
   absolute zeros in libwebp and deltas in dixie (and in libvpx); a filter
   level is clamped once after every adjustment in libwebp, twice in dixie;
   the colour-space bits are ignored by libwebp and refused by dixie.
   *For libwebp:* it is what every browser shows, and a picture that looks
   one way everywhere else and another way here is a bug report. *Against:*
   the RFC is the specification, and on these files this decoder is, by the
   letter, the one out of step. The tests hold each case to libwebp through
   frames whose headers `tests/data/vp8rewrite.py` rewrites into exactly
   those settings.
2. **libwebp's colour conversion.** VP8 stops at Y'CbCr; turning that into
   RGB is the application's business, and libwebp's is BT.601 studio swing in
   14-bit fixed point with its "fancy" chroma upsampling (a 9-3-3-1 filter,
   which equals libwebp's two-step form exactly -- a test checks every
   input). *For:* the pixels match the browsers'. *Against:* it is not the
   conversion this crate's JPEG decoder uses (JPEG's full-range, libjpeg's
   upsampling); the two formats' colours are each right for their format.
3. **Libwebp's answer on corrupt files, too.** A truncated or damaged file
   should fail, or show, as it does in a browser. That took three details a
   valid file never reaches: a frame's decoder is handed its chunk *with* the
   padding byte of an odd-length chunk, as libwebp's demuxer hands it, so a
   frame one byte short still decodes; the boolean decoder keeps libwebp's
   exact formulation, down to reading a coefficient's sign as its
   `VP8GetSigned` does -- which differs from an ordinary even-odds decision
   once a corrupt partition (a first byte of `0xFF`) has pushed the coder's
   value past its range; and the lossless decoder flags running out where
   libwebp's reader does (never within the first 64 bits it loads), and, for
   an alpha plane libwebp decodes a byte per pixel, forgives the last
   symbol's overrun as libwebp does. *For:* 6,981 corrupted and truncated
   files decode here exactly as in libwebp, or fail where it fails, which
   makes the comparison a regression test for anything done to the decoder
   later (a speed-up, say). *Against:* these are libwebp's implementation
   details, not the format's; they cost a few careful lines each, and
   comments saying why. Where libwebp's SIMD inverse DCT computes in 16 bits
   (coefficients no valid stream has) this follows its C code, and the
   comparisons found no file on which that shows.
4. **Reconstructed whole, filtered a row behind.** Prediction reads the frame
   before loop filtering; libwebp keeps copies of the unfiltered edges so it
   can filter as it goes. Here the planes are the frame, and a row of
   macroblocks is filtered once the row below it has been reconstructed --
   which is when nothing still needs it unfiltered, since a row's filter
   reaches three samples up and none down. *For:* no edge caches to get
   wrong. *Against:* one more pass over each row's samples; not measurable
   next to the rest.

### How it is held

`tests/webp.rs` compares 36 fixtures pixel for pixel with Pillow's decode
(libwebp 1.6): libwebp's own output at several sizes and settings
(segments, the simple and normal filters and none, sharpness, 1 to 8
partitions, the skip flag, odd sizes down to 1x1), libvpx's, alpha planes
under each filter raw and compressed, headers rewritten into what no encoder
writes, one corrupt frame and one frame cut short. A census test in
`lossy.rs` parses the fixtures with the decoder's own functions and fails if
they stop using any feature: every segmentation form, filter, high-variance
threshold, partition count, quantiser delta, prediction mode and coefficient
token. The transforms, predictors and loop filter are each checked against
the RFC's reference code over thousands of inputs.

Outside the suite, 701 cut files and 6,280 bit-flipped ones were compared
with libwebp; every difference found was one of the details in item 3.

*Addendum, same day.* The lossless decoder's bit reader is now a port of
libwebp's `VP8LBitReader` -- its 64-bit window, the refills at the points
libwebp refills, symbol reads that move the position without looking, the end
flagged where libwebp flags it -- and its pixel loops check where libwebp's
do, including the byte-per-pixel loop libwebp uses for colour-indexed alpha
planes. That closed the last difference the comparisons had found (an alpha
plane whose last symbol read stale bits from the window) and three more that
a further 4,500 corrupted lossless files turned up: a simple prefix code
naming a symbol past its alphabet names nothing, as in libwebp; a lossless
picture whose header says it has no alpha is shown opaque, as Pillow and
Firefox (through `WebPGetFeatures`) show it; and a lossy frame's `ALPH` chunk
under a `VP8X` header without the alpha flag is dropped unread, as libwebp's
demuxer drops it. What remains are files whose RIFF structure libwebp's
demuxer refuses and this decoder's chunk walk accepts; porting the demuxer is
the next step, and animation needs it anyway (done: §1313). Four more
fixtures hold these cases.

### Measured

Release, Windows, best of five: a 2000x1500 photograph at quality 80 decodes
in 181 ms, where libwebp (through Pillow) takes 97; 4000x5333, 1.53 s against
0.86. About half libwebp's speed, like the lossless decoder; what to do about
it is in `known-issues.md`.
