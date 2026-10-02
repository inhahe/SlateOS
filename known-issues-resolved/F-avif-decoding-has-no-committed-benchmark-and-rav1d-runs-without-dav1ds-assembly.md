### [F] AVIF decoding has no committed benchmark, and rav1d runs without dav1d's assembly -- 2026-09-27

**Status:** HALF FIXED on `lane-f` 2026-09-27. Part (1) is done:
`avif::decode::bench::bench_avif_decode` (run with `cargo test -p imagecodec
--release --lib -- --ignored --nocapture bench_avif`) over four
photograph-like inputs from `tests/data/generate_avif_bench.py`, either side
of `THREADED_PIXELS` and one deep. First figures, one thread, best of five on
a loaded machine: 640x480 30 ms, 1920x1080 199 ms, 2560x1440 354 ms, 10-bit
4:4:4 1080p 353 ms -- against Pillow's dav1d with its assembly at 22, 88,
161 and 202 ms. The threaded figures of both decoders swung either way under
the load, so `THREADED_PIXELS` wants an idle-machine run before it is
retuned. Part (2) waits on the operator: `open-questions.md` F-Q4 (bring in
dav1d's assembly, or write SIMD in Rust).

**In short:** AVIF pictures decode correctly but more slowly than in Chrome
or Pillow. rav1d, the AV1 decoder in `gui/video/rav1d`, was vendored as pure
Rust without dav1d's hand-written assembly, and single-threaded it takes
1.3 to 2.3 times as long as Pillow's dav1d on the same files (a 1204x800
photograph: 108 ms against 52 ms; a 2048x1536 grid: 1.65 s against 0.72 s).

**What there is.** `Cargo.toml` builds rav1d and `imagecodec` at `-O3` in
release (the workspace default is `-Os`), justified by single timing runs on a
machine shared with five other build lanes; `gui/imagecodec/src/avif/decode.rs`
decodes tiles under two megapixels on the calling thread (`THREADED_PIXELS`),
because starting rav1d's workers costs about 5 ms and a still picture gives
them little to share -- Pillow's dav1d was slower with ten threads than with
one on the same files.

**The proper fix, in two parts.** (1) A committed benchmark -- AVIF decode
at a few sizes and depths, best of several runs, single- and multi-threaded
-- so the `-O3` override and `THREADED_PIXELS` rest on measurements anyone can
repeat (`performance-targets.md` asks for one on every hot path). (2) SIMD for
rav1d's hottest DSP routines (motion compensation, inverse transforms, loop
filter, CDEF, loop restoration), with `std::arch` intrinsics under runtime CPU
detection, checked sample-for-sample against the scalar code. Part 2 is the
large one; part 1 should come first so it can measure part 2.
