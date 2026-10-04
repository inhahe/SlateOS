### [F] A few rare kinds of video convert to pixels too slowly to play at 1080p -- 2026-10-04

**Status:** OPEN (lane F) -- measured; the fix is known and lane F's.

**In short:** turning a decoded picture into pixels is fast for ordinary
video -- every frame is now split into bands converted on all the machine's
cores -- but a few rare kinds of video take a slow, unvectorised path:
12-bit video, and the old or unusual colour recipes (SMPTE 240M, FCC, YCgCo)
that libavif converts in floating point. At 1080p those take 30 to 90
milliseconds a frame on one core, so even split across cores they can hold a
player below real time on a machine with few cores.

**Measured** (one thread; an i7-8700K shared with other work;
`gui/video/yuv/tests/bench.rs`):

| What | Per 1080p frame |
|---|---|
| 8-bit 4:2:0 BT.709 / BT.601 (most video) | 8.3 ms -- libavif with libyuv's C takes 18.2, with its x86 SIMD 5.9 |
| 10-bit 4:2:0 (HDR, BT.2020) | 11.4 ms |
| 12-bit 4:2:0 (libyuv's nearest-neighbour I012) | 29.8 ms |
| libavif's floating-point fast path (FCC, YCgCo, the identity matrix deeper than 8 bits) | 31.2 ms |
| libavif's floating-point slow path (SMPTE 240M, FCC subsampled) | 91.7 ms |

`gui/video/codec` divides each by the cores it can use: the ordinary 8.3 ms
was 3.7 ms on twelve cores while a boot test kept most of them busy
(`picture.rs`'s `bench_bands`), and less on an idle machine.

**Where.** `gui/video/yuv/src/convert.rs` (`row_212`, I012's row) and
`gui/video/yuv/src/reformat.rs` (`fast_path`, `slow_path`).

**The proper fix.** Rewrite those inner loops as `vp9` writes its filters --
fixed widths, chunks the compiler can see are in bounds, no per-pixel
lookups through `Option` -- keeping the arithmetic, which the AVIF fixtures
(Pillow) and `gui/video/codec`'s fixtures (libavif) would hold exact; the
float paths' per-pixel table lookups and four-tap chroma reads are where
the time goes.

**What happens until then.** Ordinary video converts in a few milliseconds
a frame. Video in the rare paths above plays below real time at 1080p on a
machine with few cores.

**History.** Filed the same day as "1080p video converts on one thread, and
can fall below real time"; the one-thread half was fixed by converting in
bands (`yuv::reformat::to_argb_rows`, every band bit-exact to the whole
picture -- tested exhaustively), leaving the rare paths.
