### [F] 1080p video converts to pixels on one thread, and can fall below real time -- 2026-10-04

**Status:** OPEN (lane F) -- measured; the fixes are known and lane F's.

**In short:** turning a decoded 1080p picture into pixels takes about 8 to 10
milliseconds on one core for ordinary video, and much longer for a few rare
kinds. Where the decoder can use several cores (most real VP9 and AV1 at that
size) the total stays well ahead of the frame rate; where it can use only one
(VP9 encoded in a single tile column, as some older encoders do), 1080p plays
at about 22 frames a second where 24 or 30 are needed.

**Measured** (one thread converting; an i7-8700K shared with other work;
`gui/video/yuv/tests/bench.rs`, `gui/video/codec/tests/bench.rs`):

| What | Per 1080p frame |
|---|---|
| 8-bit 4:2:0 BT.709 / BT.601 (most video) | 8.3 ms -- libavif with libyuv's C takes 18.2, with its x86 SIMD 5.9 |
| 10-bit 4:2:0 (HDR, BT.2020) | 11.4 ms |
| 12-bit 4:2:0 (libyuv's nearest-neighbour I012) | 29.8 ms |
| libavif's floating-point fast path (FCC, YCgCo, the identity matrix deeper than 8 bits) | 31.2 ms |
| libavif's floating-point slow path (SMPTE 240M and FCC subsampled) | 91.7 ms |
| libvpx's 1080p film in one tile column, through `Video` | 28.6 frames a second decoded; 22.2 decoded and converted |
| libvpx's 1080p in 4x4 tiles | 80.6 decoded; 48.8 decoded and converted |

**Where.** `gui/video/yuv/src/reformat.rs` and `convert.rs` (the
conversion), `gui/video/codec/src/picture.rs` (which calls it once per frame,
on the caller's thread).

**The proper fix**, two parts, each keeping every pixel as it is:
1. **Convert in bands, on several threads.** Every path's rows depend only
   on their own luma rows and the chroma rows around them: libyuv's bilinear
   4:2:0 works in pairs of rows sharing two chroma rows, so bands that start
   on a pair keep its pixels exactly; libavif's float paths are per pixel.
   `reformat::to_argb_into` would take a band, and the caller split the
   picture across a thread pool. At six cores the ordinary path's 8 ms
   becomes under 2.
2. **The rare slow paths**: libyuv's I012 and libavif's float loops are not
   written for the compiler to vectorise (the per-pixel float slow path above
   all). Rewriting their inner loops as `vp9` writes its filters -- fixed
   widths, no bounds checks inside -- keeps the arithmetic and should bring
   them near the ordinary path.

A player can already overlap conversion with decoding: `Video::next_picture`
on one thread, `Video::convert` (or `Picture::to_frame`) on another --
`Picture` is `Send`.

**What happens until then.** Ordinary video at 1080p plays in real time
where its decoder has tiles to share, which is nearly all of it; a single-tile
1080p VP9 drops a few frames a second; video in the rare paths above plays
well below real time at 1080p.
