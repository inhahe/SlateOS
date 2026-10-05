### [F] VP8 of one token partition decodes at about 60% of libvpx's SIMD speed -- 2026-10-05

**Status:** OPEN (lane F) -- the first of two fixes is in (2026-10-05); the
larger part of the second waits on `open-questions/F-Q5.md` (SIMD needs
`unsafe`).

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

**What remains: fewer instructions in the hot loops** -- and the largest
part waits on `open-questions/F-Q5.md`. By callgrind (20 frames of the 1080p
stream, one thread, 2.01 G instructions), the loop filter's edges are a
third of the whole (673 M), and their arithmetic, which the compiler
already runs sixteen pixels at a time, is the small part of that: about
250 M is turning the pixels across a vertical edge into lanes and back,
byte by byte, which libvpx's SSE2 does in some forty register shuffles.
Three ways to get those shuffles without `unsafe` were measured on one
16x8 block and failed: byte shuffles of 16-byte arrays (the compiler kept
them scalar: 484 instructions), shifts and masks on eight-byte words (525
executed against the byte copy's 428), and the SSE2 intrinsics themselves,
which Rust 1.95 lets safe code name only from a function marked as needing
SSE2 -- itself callable only with `unsafe`, though every x86-64 processor
has SSE2. F-Q5's option A, extended to VP8, would allow it. Motion
compensation (about 460 M) and the coefficients (about 220 M) follow.

A third pipeline stage -- coefficients on one thread, prediction and
reconstruction on another -- would also take the macroblocks' 64% apart on
two or three cores, at the cost of a coefficient buffer per macroblock in
flight. Not done: a one-partition 1080p film already decodes at 55-85
frames a second on one core (by the machine's load), about 1.3 times that
on two.
