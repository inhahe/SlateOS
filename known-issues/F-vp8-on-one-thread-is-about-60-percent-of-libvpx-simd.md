### [F] VP8 of one token partition decodes at about 60% of libvpx's SIMD speed -- 2026-10-05

**Status:** OPEN (lane F) -- measured; the fixes are known and lane F's.

**In short:** most VP8 video is coded in one "token partition" (one stream
of coefficients per frame), which no decoder can split among processor
cores the way it splits frames of several -- libvpx decodes those on one
core too. On one core this port is 2.7 times as fast as libvpx built from
its C, but about 60% of the speed of libvpx with its hand-written SIMD,
what browsers ship. 1080p plays (65-85 frames a second here, against 25-30
needed), so nothing is wrong in what is shown; a slow machine has less
room to spare than libvpx gives it.

**Measured** (2026-10-05, 217 frames of 1080p film at 5 Mbit/s,
`gui/video/vp8/tests/bench.rs`'s stream; an i7-8700K at about 90% load from
other work, so each figure is the best of three, measured back to back):

| Decoder, one thread | Frames a second | Instructions, first 20 frames |
|---|---|---|
| this port (`-O3`) | 65-78 | 1.99 G |
| libvpx 1.17.0, plain C | 23-30 | 6.46 G |
| libvpx 1.17.0, x86 SIMD | 97-134 | 0.86 G |

Where the time goes, by timers around each part (one thread): the
macroblocks themselves -- coefficients, prediction, inverse transforms --
64%; the loop filter 32%; the frame header and modes, 6% (they precede the
rows, in libvpx too).

**The proper fixes,** in the order they pay:

1. **The loop filter on a thread of its own, a row behind.** Filtering row
   `r - 1` touches rows `r - 2` and `r - 1` only, and decoding row `r + 1`
   reads row `r`'s unfiltered bottom pixels and writes row `r + 1`, so the
   two can run at once on different rows -- the same pixels, in a pipeline
   of two stages, with the row hand-over `threading.rs` already does
   between rows. libvpx does not do this; it would make one-partition VP8
   up to about 1.45 times as fast on two cores (the filter's third of the
   time taken off the decoding thread).
2. **Fewer instructions in the hot loops:** libvpx's SSE2 loop filter
   transposes sixteen rows at a time in registers where this gathers bytes
   (`loopfilter.rs`), and its six-tap filter keeps a block in registers
   across both passes (`inter.rs`). Hand-written SIMD needs `unsafe`, which
   the crate forbids; the route is loops shaped so the compiler emits what
   libvpx's assembly does, as the current ones are, checked by callgrind.
