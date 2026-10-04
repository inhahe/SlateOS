## 1339. VP9 is libvpx's decoder ported to Rust, decoder first, checked frame by frame against libvpx's conformance vectors

**Date:** 2026-10-03
**Lane:** F
**Decided by:** Claude (autonomous), carrying out the operator's §1332 ("do
vp9 ... preferably find ... cpu fallback either way - multithreaded").

**In short:** SlateOS will decode VP9 video, the format most WebM files and
most of YouTube use, with a Rust translation of Google's reference
decoder (libvpx). It is checked against libvpx's own test suite: 314 videos,
each with a fingerprint of every decoded frame, so "it works" means it
produces exactly the same pixels as the reference. The encoder, which
remote desktop needs to film the screen, comes after, using the decoder as
its check. The first program to benefit is the video player, which today
says "nothing here decodes video".

**What was decided.**

- **A Rust port, not libvpx's C compiled in.** The tree builds with no C
  compiler -- not for host tests, not for the Linux lint, not for SlateOS
  (`gui/tsgrammar`'s reason for converting tree-sitter's C rather than
  compiling it). Adding one for a codec that parses hostile input would also
  bring libvpx's C memory-safety record with it.
  CVE-2023-5217, exploited in the wild, was in libvpx's VP8 encoder. The
  port is of libvpx v1.17.0's C reference paths, function for function,
  in safe Rust: `gui/video/vp9`, a crate of its own beside `rav1d`.
- **Decoder first.** It has a user today (lane E's video player and media
  tools), it is deterministic, so correctness is provable to the bit, and it
  is the test oracle for the encoder: anything the encoder writes must decode.
- **The test is libvpx's.** `test/test_vector_test.cc` decodes each vector
  and compares an MD5 of every output frame with the vector's `.md5` file.
  The port does the same, with the same MD5 rule (`test/md5_helper.h`):
  visible rows only, chroma rounded up, two bytes a sample above 8 bits. A
  subset of small vectors is committed. The full set of 314 (26 MB) is
  fetched into `target/` and run when present.
- **Threads after correctness.** VP9 frames split into tiles that decode
  independently, and libvpx threads by tile columns and by loop-filter rows.
  The thread count is the machine's core count, as §1332 asks. It comes once
  single-threaded decoding matches every vector, so threading cannot hide a
  decoding bug.
- **No hand-written SIMD for now.** Like `rav1d` here, the speed is the
  compiler's. Rust's intrinsics need no assembler, so this is not
  `open-questions.md` F-Q4 (dav1d's assembly), and a committed benchmark
  decides whether they are worth it.
- **A hostile stream never panics.** Every index derived from the stream is
  bounded or checked, and arithmetic that a crafted stream could overflow
  wraps, as libvpx's C does, so a bad file decodes to garbage or an error,
  never a crash.

**Status, 2026-10-03: the decoder is done.** Every picture of all 314
vectors matches libvpx's MD5 -- every profile, bit depth and subsampling.
Mutating the loop filter's rounding, the motion compensation's, the motion
vector search or the probability adaptation each fails most vectors, so the
test reaches every part. Two places where the port deliberately does not do
what libvpx's C does:

- **One set of inverse transforms, not two.** libvpx keeps an 8-bit copy of
  each transform that narrows intermediate values to 16 bits, and a
  high-bit-depth copy that keeps 32 and outputs zero for inputs past 2^25.
  On every stream the VP9 specification allows, the two compute the same
  numbers; they differ only where a stream breaks the specification's range
  rule, and there libvpx's own SIMD builds differ from its C (they saturate
  where it wraps). The port keeps one 32-bit copy that wraps: libvpx's
  result on every valid stream, a deterministic one on any other. A test
  replays libvpx's C transforms (`tools/idct_reference.c`) on seeded blocks
  at all three bit depths and matches every hash.
- **A size limit.** libvpx decodes a frame of any size a header names, which
  lets a hostile stream ask for gigabytes. The port refuses frames over
  VP9's level 6.2 (8192 x 4352 samples) unless the caller raises the limit.

**Speed, 2026-10-03.** The benchmark (`tests/bench.rs`) decided the first
round. Counted with callgrind on 30 frames of libvpx's 1080p vector, the
port began at 42.7 G instructions, 84% of them in motion compensation (a
third of all in `memset`, from buffers zeroed per block). With its
filters written so the compiler vectorises them for baseline x86-64 --
block widths as constants, 8-bit sums taken exactly in 16-bit lanes, the
loop filter eight lines at a time -- it takes 6.57 G; libvpx's C takes
13.8 G. On one thread, 1080p decodes at 32 frames a second against
libvpx's C at 17 and its SIMD at 77 (an i7-8700K). Tile columns now
decode on threads, as libvpx's do. Two things from it:

- **The crate is built at `opt-level = 3`** (root `Cargo.toml`): at the
  workspace's `-Os` the same frames take 21.98 G instructions, 3.3 times
  as many, since `-Os` undoes the unrolling the filters are written for.
- **The rest of the gap to libvpx's SIMD is instructions SSE2 lacks**
  (SSSE3's `pmaddubsw` multiplies bytes in pairs; AVX2 doubles the width).
  Using them means choosing them at run time, which in Rust needs `unsafe`
  -- in a crate that parses hostile input and forbids it today. Put to the
  operator as `open-questions/F-Q5.md` once threads had given what they
  could, rather than changed quietly.

**The encoder, begun 2026-10-03.** The port is of libvpx's realtime path,
the one `vpxenc --rt --cpu-used=8` with CBR rate control runs, since filming
a screen is a realtime job. Its test goes beyond "it decodes": libvpx's
encoder is deterministic too, so the port's frames are to be byte-identical
to `vpxenc`'s on the same input and settings (a reference encode is kept
outside the tree for that comparison). How it is built:

- **The deciding is apart from the doing.** `enc/encodeframe.rs` carries out
  decisions -- partitions, modes, transform sizes -- exactly as libvpx's
  `encode_superblock` does: predict, subtract, transform, quantise,
  reconstruct, tokenize, count. What to decide is asked of a `Decide`
  implementation. libvpx's realtime choices (variance-based partitioning,
  `vp9_pick_intra_mode`, `vp9_pick_inter_mode`) are ported as one; a test's
  random choice is another, which reaches every partition, mode and
  transform size the stream can say.
- **One set of contexts.** The contexts each decision is coded with
  (`vp9_pred_common.c`) moved out of the decoder into `context.rs`, which
  the decoder reads with and the encoder writes with. A context computed
  two ways is a stream that decodes to garbage.
- **Each piece against libvpx's C first.** The forward transforms and both
  quantisers are checked bit-exact against libvpx's C on seeded blocks
  (`tools/fdct_reference.c`, `tools/quantize_reference.c`); the probability
  updates against the decoder's reader for every pair of probabilities.
- **libvpx's structure, kept.** libvpx's encoder is one structure every
  part reaches into (`VP9_COMP`); the port keeps it as `enc/cpi.rs`'s `Cpi`,
  with rate control, cyclic refresh and the frame loop as `impl Cpi` blocks
  in modules of their own, so a function ports line for line.

**Status, 2026-10-03: the first frame is libvpx's.** The reference encode
(`tests/data/encoder/rt8.ivf`, how it was made beside it) opens with a key
frame, and the port's first frame is byte-identical to it: libvpx's one-pass
CBR rate control picks the same quantiser from the same model in the same
floating point, its variance partitioning cuts the same blocks, its realtime
intra search picks the same modes, and the coefficients, tokens, probability
updates and headers come out bit for bit. Every frame also decodes to what
the encoder says it reconstructed -- at random decisions, at seven sizes from
1x1 up, at five quantisers -- and what the encoder counts equals what the
decoder counts. Every frame is a key frame so far; inter frames come next,
and the reference's 29 others are their test.

**2026-10-04: inter frames are coded, with decisions given.** The doing half
of an inter frame is in: the reference slots and their refresh, the inter
frame's headers, prediction from the references (the decoder's own
predictor, over references whose edges are extended past the picture as
libvpx's encoder extends them, which blocks at the picture's edge predict
from), the fast quantiser libvpx's realtime path uses on inter blocks,
motion vector prediction (shared with the decoder, as the contexts are),
vector coding, sub-8x8 inter blocks, the segment map's temporal prediction,
and every probability update an inter frame's header can carry. Decisions
are still a caller's: a test draws every one at random -- references, inter
modes, vectors near and far and at both precisions, filters, segments with
every feature, skips -- and each frame of a key-then-inter stream decodes to
the encoder's reconstruction while the encoder counts what the decoder
counts, over a thousand streams in a soak. Until libvpx's inter decisions
are ported, `Encoder::encode` keeps coding every frame as a key frame.

**2026-10-04: every frame is libvpx's.** The inter decisions are in: the
speed-8 variance partitioning and its shortcut of copying the last frame's
partition, the realtime inter mode search (`vp9_pick_inter_mode`, with the
fast diamond, sub-pixel and integral-projection searches, the filter search
and the encode breakout), cyclic refresh's band of refreshed blocks and
their segments, and what libvpx learns from the source -- each superblock's
change since the last picture, scene cuts, the noise level, skin. All 30
frames of the reference encode are now byte-identical to `vpxenc`'s from the
encoder's own decisions. Two things it took:

- **A decision trace on both sides.** A differing frame says little about
  why: one block that picked another mode changes every byte after it. So a
  copy of libvpx is instrumented to log each superblock's partition and each
  candidate its mode search scored, the port logs the same lines in tests,
  and a comparer finds the first line that differs. The tools and the
  procedure are in the tree (`gui/video/vp9/tools/trace/`); on the
  reference encode the two logs agree on all 365,835 lines.
- **libvpx's stale state, reproduced.** libvpx reads state it never
  refreshed, and its decisions depend on it: the mode-info buffer it swaps
  with every frame and never clears (a check of low motion reads the vector
  left by whatever block last covered the cell, two frames ago), cost tables
  rebuilt only every eighth frame (a table never built is zeros), a mode's
  threshold carried from frame to frame. The port models each as libvpx
  holds it, with a comment at the read. Reading them as bugs and "fixing"
  them would make different frames from `vpxenc`'s, which is the one test
  the encoder has.

A second reference encode reaches what that clip never does
(`tests/data/encoder/rt8cut.ivf`): 150 pictures of 651x357 cut from two
conformance vectors -- two scene cuts, a still picture with fresh noise on
each frame, a fade to black -- so that libvpx codes its cuts at the
overshoot quantiser, raises its noise estimate to Medium, refreshes golden
three times, sees a change of light that is not motion, and partitions
blocks hanging over the picture's edges. Its trace found one difference,
at the first cut: the port capped an intra block's transform at 16x16 when
the frame coded no segments, where libvpx asks only whether the encode runs
cyclic refresh at all. With that fixed, all 150 frames are `vpxenc`'s and
the traces agree on all 867,893 lines. How the encoder keeps time is §1340.

**At every size, 2026-10-04.** At 352x288 and below libvpx partitions inter
frames differently: it searches each superblock's square partitions before
coding any of it, a small network deciding which of whole and cut to try
(`nonrd_pick_partition`, `ml_predict_var_partitioning`). That is ported, and
a third reference encode at 350x286 (`tests/data/encoder/rt8small.ivf`) is
byte-identical for all 90 frames, its traces agreeing on all 152,823 lines.
What it took, beyond the search itself:

- **The network's logarithm is glibc's FMA build** (§1341).
- **More of libvpx's state between blocks.** Searching a whole superblock
  before coding it exposes state the variance path never let anyone read:
  each search leaves its prediction in the reconstruction, which the next
  block's intra search reads as its neighbour, and leaves the winner's when
  it kept its predictions aside to reuse them; each block's mode
  information carries the "codes nothing" flag its buffer cell held two
  frames ago, which the next block's skip context reads; the cyclic refresh
  judges a block by `x->skip` as the block before it left it; and a block
  smaller than `x->max_partition_size` -- 64x64 here, 32x32 on the variance
  path -- also starts its motion search from the vector it found last.

**Alternatives.**

| | For | Against |
|---|---|---|
| Port to Rust (chosen) | no C compiler in any build; memory-safe on hostile input; the house way (`webp/lossy` is libwebp's VP8 ported) | the most work |
| Compile libvpx's C | least work; libvpx's SIMD for free | a C toolchain in every build, and C parsing hostile video |
| Machine-translate with c2rust, as rav1d began | fast to a first decode | 100k lines of `unsafe` Rust that are C in disguise, to clean up by hand anyway |
| Write a VP9 decoder from the specification | no inherited structure | the "write" §1332 prefers to avoid, and nothing to be bit-exact against except libvpx |
