### [F] VP8 of one token partition decodes at about 60% of libvpx's SIMD speed -- 2026-10-05

**Status:** OPEN (lane F) -- the first of two fixes is in (2026-10-05); the
second is known and lane F's.

**In short:** most VP8 video is coded in one "token partition" (one stream
of coefficients per frame), which no decoder can split among processor
cores the way it splits frames of several -- libvpx decodes those on one
core. On one core this port is 2.7 times as fast as libvpx built from its
C, but about 60% of the speed of libvpx with its hand-written SIMD, what
browsers ship. It now gives such a frame's loop filter a second core,
which brings it to about 80% of libvpx's single-core SIMD at 1080p. 1080p
plays (65-85 frames a second here on one core, against 25-30 needed), so
nothing is wrong in what is shown; a slow machine has less room to spare
than libvpx gives it.

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

**Done: the loop filter on a thread of its own, a row behind**
(`gui/video/vp8/src/pipeline.rs`, design-decisions §1357). The decoding
thread decodes rows into bands, keeping each row's unfiltered bottom
pixels for the row below, so it never reads the frame; a second thread
copies each row in, filters it and fills its borders. The same pictures,
bit for bit. Measured with the benchmark at high priority: 1080p 1.31
times as fast on two threads as on one, 720p 1.25, 640x360 1.17 -- short
of the 1.45 the filter's third of the work allows, because decoding a row
is still on one thread with the frame header and modes before it.

**What remains: fewer instructions in the hot loops.** libvpx's SSE2 loop
filter transposes sixteen rows at a time in registers where this gathers
bytes (`loopfilter.rs`), and its six-tap filter keeps a block in registers
across both passes (`inter.rs`). Hand-written SIMD needs `unsafe`, which
the crate forbids; the route is loops shaped so that the compiler emits
what libvpx's assembly does, as the current ones are, checked by
callgrind. A third pipeline stage -- coefficients on one thread, prediction
and reconstruction on another -- would also take the macroblocks' 64% apart,
at the cost of a coefficient buffer per macroblock in flight.
