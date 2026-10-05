## 1344. The AVIF decoder follows libyuv's C where its x86 SIMD computes other pixels

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** libyuv, the library libavif hands pictures to for resizing and
for turning YUV into RGB, keeps each routine twice: in plain C, and in
hand-written x86 vector code (SIMD: one instruction working on many pixels at
once). The two are meant to agree, and almost always do. In three rare
situations they give slightly different pixels, and which one a program shows
depends on how its copy of libyuv was compiled -- Pillow on Windows runs the
C, Chrome and Linux builds run the vector code. SlateOS's AVIF decoder
computes what the C computes: libyuv's reference, the same answer on every
machine, and what the tests' reference decoder (Pillow on Windows) shows.

**Where they differ.** Each was found by building libyuv both ways and
comparing; all three need an unusual file.

| Situation | C | x86 SIMD |
|---|---|---|
| Undoing premultiplied alpha (`ARGBUnattenuate`; files with a `prem` reference) | divides in 8.8 fixed point, rounding down | widens by repeating the byte, rounds up about half the time |
| An 8-bit frame declared at exactly 3/4 or 3/8 of its coded size on both sides | its box rows' roundings | SSSE3's, which round otherwise |
| An 8-bit frame declared at under 1/257 of its coded height, over bright samples | 16-bit column sums wrap: white comes out 32 | SSE2 saturates: white comes out 95 |

(The last is wrong both ways; libyuv's box filter also darkens any very large
box, by dividing with a truncated reciprocal -- which both share, and which is
ported.)

**What was decided.** Port the C as an x86-64 build compiles it, which
includes the forms libyuv's C keeps for x86 only (the scaler's 7-bit
horizontal blend, the conversions' constant tables read in the x86 order):
`gui/video/yuv/src/convert.rs`, `gui/video/yuv/src/scale.rs` (moved there
from `gui/imagecodec/src/avif/` on 2026-10-04, so that video uses them too).

**Alternatives.**

| | For | Against |
|---|---|---|
| libyuv's C (chosen) | libyuv's reference; one answer on every machine; what Pillow's Windows build computes, so every fixture is checked byte for byte | differs from Chrome on x86 in the three situations above |
| libyuv's x86 SIMD | what Chrome and Linux builds show on x86 | ARM builds compute yet another answer (NEON, and a C blend of their own), so "Chrome's pixels" are not one thing; the fixtures would need a Linux Pillow; each SIMD routine's quirks would need porting from assembly |

**How to reverse.** Each difference is one function: `unattenuate`, `box34`
and `box38` for 8-bit samples, and `Scaled::add_to_sum` for `u8` (saturate
instead of wrap).

**Revisit when** a real file shows a visible difference from Chrome, or the
fixtures' reference changes to a decoder that runs libyuv's SIMD.
