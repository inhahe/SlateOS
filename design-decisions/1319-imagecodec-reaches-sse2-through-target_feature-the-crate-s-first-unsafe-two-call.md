## 1319. imagecodec reaches SSE2 through `#[target_feature]`: the crate's first `unsafe`, two call sites, each exact to the scalar code beside it

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** a JPEG photograph took three times as long to decode here as
in a browser. The two steps that cost the most after the entropy decoding --
undoing the compression's transform, and converting the colours -- are
exactly the work a processor's vector instructions exist for, and the
compiler would not use them on its own for this code. So the decoder now
uses them directly, which Rust allows only through `unsafe`: two lines, each
a call into a function that may run only on a processor with SSE2 -- every
64-bit PC has it, and the build turns it on. The pictures are unchanged to
the bit; a 21-megapixel photograph decodes in about 0.84 billion cycles
where it took 1.80 (libjpeg-turbo: 0.62).

**Decision.** `jpeg/idct/sse2.rs` and `jpeg/color/sse2.rs` are
`#[target_feature(enable = "sse2")]` functions written with `core::arch`
intrinsics, which are safe to call inside such a function; values go in and
out by value, so there are no pointers. `idct::inverse` and
`color::ycc_argb` reach them through one `unsafe` call each, compiled only
under `cfg(all(target_arch = "x86_64", target_feature = "sse2"))`: the
SlateOS target enables SSE2 for the whole build, and a build without it
(`x86_64-unknown-none`) compiles the scalar code alone.

Exactness is the point, and each is held to the scalar code, not merely
near it:

* The inverse DCT follows libjpeg's C arithmetic, not libjpeg-turbo's SIMD.
  It checks every dequantised coefficient and every value between its two
  passes against a bound (16,383) under which nothing it computes can
  overflow its lane, and hands the block to the 64-bit scalar transform
  when one is past it -- a corrupt file's block; no encoder makes one. Its
  output stage masks to ten bits and maps them as the C code's range-limit
  table does, where libjpeg-turbo's SIMD saturates.
* The colour conversion computes libjpeg's table entries arithmetically,
  each constant over 2^16 split so every multiplication is 16-bit; a test
  compares all 2^24 inputs with the tables.

**Alternatives.**

| | For | Against |
|---|---|---|
| Portable code the compiler vectorises (what the WebP decoder does) | no `unsafe` | tried first: the transform over `[i32; 8]` lanes compiled to scalar multiplies and array copies and was *slower* than the scalar code; the conversion compiled to a branchy scalar loop at ~16 cycles a pixel. SSE2 has no 32-bit lane multiply, and the compiler did not find `pmaddwd` |
| Stay scalar | nothing to audit | roughly 0.7 billion cycles more per photograph; the decode is what an image viewer's wait is made of |
| Match libjpeg-turbo's SIMD, not its C code | Chrome's and Pillow's arithmetic on x86, corrupt files included | the C code is the reference that agrees with itself on every machine, and what this port and its oracle follow (§1318) |

**Measured** (thread cycles, 4000x5333 4:2:0, billions): 1.80 before;
1.62 with the entropy decoder's bulk fills; 1.11 with the SSE2 transform;
1.02 with the conversion written straight into `0xAARRGGBB` pixels (no row
of bytes between); 0.86 with the SSE2 conversion; 0.84 with the bit buffer
held in locals.
Held to libjpeg-turbo 3.1.1's C build on its 402 seeds and 4,000 mutants at
every reduced size, to libtiff on 3,000 mutated JPEG and old-JPEG TIFFs,
and by tests that fail if a constant or the bound is wrong (both were
tried).

**How to reverse.** Delete the two `sse2.rs` modules and the two `cfg`
blocks that call them; the scalar code is still there and still the
fallback.
