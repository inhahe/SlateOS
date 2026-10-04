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
  -- in a crate that parses hostile input and forbids it today. Not taken
  up while threads still have more to give; if it is, it is a question for
  the operator, not a change to make quietly.

**Alternatives.**

| | For | Against |
|---|---|---|
| Port to Rust (chosen) | no C compiler in any build; memory-safe on hostile input; the house way (`webp/lossy` is libwebp's VP8 ported) | the most work |
| Compile libvpx's C | least work; libvpx's SIMD for free | a C toolchain in every build, and C parsing hostile video |
| Machine-translate with c2rust, as rav1d began | fast to a first decode | 100k lines of `unsafe` Rust that are C in disguise, to clean up by hand anyway |
| Write a VP9 decoder from the specification | no inherited structure | the "write" §1332 prefers to avoid, and nothing to be bit-exact against except libvpx |
