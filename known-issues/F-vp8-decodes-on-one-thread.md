### [F] VP8 decodes on one thread, at about a third of libvpx's SIMD speed -- 2026-10-04

**Status:** OPEN (lane F) -- measured; the fix is known and lane F's.

**In short:** VP8 video (old WebM files, and the video of many video calls)
decodes correctly but on one processor core only. A 1080p film decodes at
30-40 frames a second on this machine -- enough to play it, faster than
libvpx built without its hand-written SIMD, but about a third of libvpx's
speed with it, and libvpx can also share a frame's rows among several
cores, which this cannot. Nothing is wrong in what is shown; a slower
machine may drop frames of 1080p VP8 that libvpx would not.

**Measured** (`gui/video/vp8/tests/bench.rs`, 217 frames of 1080p film at
5 Mbit/s, one thread, an i7-8700K shared with other work, so the frame
rates swing by up to a factor of two between runs; callgrind's counts over
the first 20 frames do not):

| Decoder | Frames a second | Instructions, 20 frames |
|---|---|---|
| this port (`-O3`) | 30.8 (Windows), 40.0 (WSL, best) | 1.99 G |
| libvpx 1.17.0, plain C | 24.9 | 6.46 G |
| libvpx 1.17.0, x86 SIMD | 110.2 | 0.86 G |

**Where.** `gui/video/vp8/src/decodeframe.rs`, `decode_mb_rows`: one row of
macroblocks after another. What remains on one thread, by callgrind: the
loop filter (`loopfilter.rs`, about a third), motion compensation
(`inter.rs`), and the coefficients and modes, which are bool-decoded and
cannot be vectorised.

**The proper fix.** Port libvpx's row threads, `vp8/decoder/threading.c`'s
`vp8mt_decode_mb_rows`: when a frame has several token partitions, each
thread decodes every Nth row, a row waiting for the one above it to be a
few macroblocks ahead, with the rows' unfiltered edges saved for intra
prediction so that each row can be loop-filtered as soon as it is done.
Its pictures are the same as the single-threaded decoder's, so the
committed vectors (several of which have token partitions) and the damage
cases test it as they stand. A frame of one partition -- most encoders'
default -- stays on one thread in libvpx too; for those, a faster loop
filter (libvpx's SSE2 transposes sixteen rows at a time in registers,
where this gathers bytes) is what is left.
