### [F] MP3 decodes at about 1.25 times minimp3's cost -- 2026-10-04

**Status:** OPEN — lane F's; tech debt, not a bug.

**In short:** the MP3 decoder (`gui/video/mp3`) gives minimp3's samples to
the bit, a little more slowly: about 150 million instructions to minimp3's
120 on 12.5 s of 192 kbit/s stereo (callgrind in WSL, each harness's own
hashing taken out), and about 600 times faster than real time against
minimp3's 900 on a 60-second file. Nothing a user does is held up by it: a
four-minute song decodes in under half a second.

**Where it is** (callgrind, millions of instructions, inclusive):

| | port | minimp3 (GCC -O2, `MINIMP3_NO_SIMD`) |
|---|---|---|
| synthesis filterbank (`synth::synth_granule`) | 100 | 54 |
| Layer III decoding and IMDCT (`l3::decode`, `imdct36`) | 46 | 63 |

All of the difference is the synthesis window's sums: each row adds sixteen
weighted rows of four floats into two sums of four. GCC vectorises
minimp3's "scalar" code across the four columns (`mulps`, `addps`), each
column's order kept; LLVM pairs the steps instead (two rows' products in
one register, `unpcklps` to build them), keeps the four columns' chains in
scalar registers and spills: 112 `mulss` and 8 `mulps` in the function.
Three restructurings that leave the arithmetic as it is changed nothing
(100.6, 100.2, 100.3 million): fixed-size arrays for the window and the
rows, the steps written out with no branch, the rows gathered into local
arrays before the sums. The saturating `as i16` of the output adds about
five instructions a sample (cvttss2si plus clamps), a further 5 million.

**The proper fix:** the sums in SSE, four columns a register, each column's
operations in minimp3's order -- the order minimp3's own SSE build sums them
in, so the same floats to the bit (a test holding them to the scalar sums
on random rows, as a first attempt here had). The intrinsics need an
`unsafe` block where they are called, and the crate is
`#![forbid(unsafe_code)]`: a calling function without
`#[target_feature(enable = "sse")]` may not call them safely even where the
target has SSE, and a function with it may not be called safely from one
without. So the fix is either a reviewed `unsafe` island (one function,
`// SAFETY: SSE is in the x86-64 baseline; the path is compiled only where
target_feature = "sse"`) with the scalar sums kept for other targets, or
waiting for `core::simd`, which is not stable. Re-measure with callgrind in
WSL (`~/mp3bench` held the comparison: the patched reference and the crate's
`answer`-style decoding of the same file, identical outputs) after the
change; `tests/fixtures.rs` and the ignored `tests/vectors.rs` hold every
change to minimp3's samples.
