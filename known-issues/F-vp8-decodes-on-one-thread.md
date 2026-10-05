### [F] VP8 decodes on one thread, at about a third of libvpx's SIMD speed -- 2026-10-04 -- **FIXED 2026-10-05**

**Status:** FIXED 2026-10-05 (lane F). libvpx's row threads are ported:
a frame of several token partitions decodes its macroblock rows on up to
`Decoder::threads()` threads (a new decoder: one per core), each row a few
macroblocks behind the one above, making the single thread's pictures bit
for bit (`gui/video/vp8/src/threading.rs`; design-decisions §1356). What
remains -- the speed of a frame of one partition, which libvpx decodes on
one thread too -- is its own entry:
`known-issues/F-vp8-on-one-thread-is-about-60-percent-of-libvpx-simd.md`.

**Checked:** the 62 libvpx vectors at 1, 2, 3, 4 and 8 threads; the 446
damage cases at 1, 3 and 8; every committed vector and random damage
decoded in step on 1, 2, 3 and 8 threads with every buffer compared,
borders included, after every frame; and the row decoder alone on random
modes, wild vectors and noise partitions, at 2, 3 and 8 threads against
one, including the version-3 macroblock that sends a frame back to one
thread.

**Measured** (`gui/video/vp8/tests/bench.rs`, the 1080p film coded in
eight partitions, `tools/make_bench_stream.py --vpxenc`; an i7-8700K at
about 90% load from other work, so each figure is the best of six
interleaved rounds):

| Threads | This port, fps | libvpx SIMD, fps | libvpx C, fps |
|---|---|---|---|
| 1 | 72.3 | 136.4 | 28.1 |
| 2 | 103.1 | 191.8 | 44.2 |
| 4 | 143.1 | 245.5 | 61.0 |
| 6 | 181.5 | -- | -- |
| 8 | 189.5 | 161.4 | 28.8 |

On this loaded machine libvpx's threads, which spin and yield while they
wait, ran slower on eight than on four; these sleep after a short spin.

---

**In short (as filed):** VP8 video (old WebM files, and the video of many
video calls) decodes correctly but on one processor core only. A 1080p
film decodes at 30-40 frames a second on this machine -- enough to play
it, faster than libvpx built without its hand-written SIMD, but about a
third of libvpx's speed with it, and libvpx can also share a frame's rows
among several cores, which this cannot. Nothing is wrong in what is shown;
a slower machine may drop frames of 1080p VP8 that libvpx would not.

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
