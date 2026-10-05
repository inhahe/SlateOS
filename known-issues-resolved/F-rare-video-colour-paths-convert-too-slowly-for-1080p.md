### [F] A few rare kinds of video convert to pixels too slowly to play at 1080p -- 2026-10-04 -- **FIXED 2026-10-05**

**Status:** FIXED 2026-10-05 (lane F). The slow paths' loops are rewritten
for the compiler to vectorise, every pixel the same as before:

| What | Per 1080p frame, one thread, before | After |
|---|---|---|
| 12-bit 4:2:0 (libyuv's nearest-neighbour I012) | 20.9-22.4 ms | 6.0-6.7 ms |
| libavif's floating-point fast path (FCC 4:4:4 measured) | 21.0-24.4 ms | 13.7-15.9 ms |
| libavif's floating-point slow path (SMPTE 240M 4:2:0 measured) | 60.2-71.0 ms | 26.7-32.8 ms |

(`gui/video/yuv/tests/bench.rs`, the crate before and after measured in
turn, three rounds, on an i7-8700K while two other boot tests ran; the
ordinary 8-bit path, unchanged, read 6.3-7.8 ms in both.) `gui/video/codec`
still divides each frame among the cores, so even the slow path converts
in real time at 1080p on two.

**What changed** (`gui/video/yuv/src/convert.rs`, `reformat.rs`):

1. **12-bit:** each chroma row is doubled to full width once per pair of
   rows (`up2_nearest`, in exact pairs the compiler interleaves eight at a
   time) and the 4:4:4 row runs over it, where libyuv's row took a pair of
   pixels to each chroma sample -- a shape the compiler kept to one pixel
   at a time.
2. **The floating-point paths** work a row at a time: the table values
   gathered into rows of floats first (the slow path's two chroma rows once
   a row, where libavif looks up four samples of each plane for every
   pixel), then the equations over those rows, chosen once a row.
3. **`to_byte`** (`(uint8_t)(0.5f + c * 255.0f)`) no longer uses `as`, whose
   saturating conversion the compiler will not vectorise for SSE2: it rounds
   with 2^23 and reads the integer from the bits, agreeing with the cast on
   all 2^32 floats (an ignored test checks every one, a regular one a
   sample and every boundary). Marked `#[inline(always)]`, as is
   `clamp_unit`: grown past the inliner's threshold, they had been called
   once a channel a pixel, which no loop vectorises around.

**Held to the code before it:** the per-pixel paths are kept verbatim in
the tests, and 3,000 random pictures -- every format, 8 to 16 bits, every
matrix that reaches these paths, either range, alpha straight and
premultiplied, deep samples past their depth -- convert to the same pixels,
alpha bytes included, and the same in bands; the 12-bit path against
libyuv's row on 400 more.

---

**In short (as filed):** turning a decoded picture into pixels is fast for
ordinary video -- every frame is now split into bands converted on all the
machine's cores -- but a few rare kinds of video take a slow, unvectorised
path: 12-bit video, and the old or unusual colour recipes (SMPTE 240M, FCC,
YCgCo) that libavif converts in floating point. At 1080p those take 30 to 90
milliseconds a frame on one core, so even split across cores they can hold
a player below real time on a machine with few cores.

**Measured as filed** (one thread; an i7-8700K shared with other work;
`gui/video/yuv/tests/bench.rs`):

| What | Per 1080p frame |
|---|---|
| 8-bit 4:2:0 BT.709 / BT.601 (most video) | 8.3 ms -- libavif with libyuv's C takes 18.2, with its x86 SIMD 5.9 |
| 10-bit 4:2:0 (HDR, BT.2020) | 11.4 ms |
| 12-bit 4:2:0 (libyuv's nearest-neighbour I012) | 29.8 ms |
| libavif's floating-point fast path (FCC, YCgCo, the identity matrix deeper than 8 bits) | 31.2 ms |
| libavif's floating-point slow path (SMPTE 240M, FCC subsampled) | 91.7 ms |

**History.** Filed the same day as "1080p video converts on one thread, and
can fall below real time"; the one-thread half was fixed by converting in
bands (`yuv::reformat::to_argb_rows`, every band bit-exact to the whole
picture -- tested exhaustively), leaving the rare paths.
