## 1380. HDR's AVX2 passes give the scalar passes' bits, not merely close ones

**Date:** 2026-10-10
**Lane:** F
**Decided by:** Claude (autonomous), within the operator's answer to F-Q5
(§1373: newer processor instructions behind run-time checks).

**In short:** the HDR conversion (showing HDR video and pictures as Chrome
shows them, §1379) now does its slowest part eight pixels at a time on
processors with AVX2, which almost every x86-64 PC since 2013 has. Done that
way it could have been a little different from the one-pixel-at-a-time code
-- a pixel one shade off here and there. It is not: the fast code computes
exactly what the slow code does, bit for bit, and tests hold it to that. The
price is some speed left on the table.

**Context.** The conversion spends most of its time in six table lookups a
pixel (the transfer curves, HLG's OOTF power, the tone map's gain, the 8-bit
encoding). AVX2's gather instruction does eight lookups at once. Vector code
usually differs from scalar code in its last bits: a fused multiply-add
rounds once where the scalar code rounds twice, a vector `exp2` is a
different approximation from libm's, operations get reordered for speed. On
8-bit output most such differences vanish, but some straddle a rounding
boundary and change a pixel by one.

**Decision.** `gui/video/yuv/src/managed/avx2.rs` (first `hdr/avx2.rs`)
computes each table pass operation for operation as the scalar pass does:
no fused multiply-add, the
same order, the scalar cast's saturation reproduced lane by lane (NaN and
negative indices to 0, large ones to the last), and where the scalar code
calls libm's `exp2f` -- the tone map's curved middle -- the lanes there go
through the scalar code itself. Remainders shorter than eight do too. Two
tests hold the two ways to the same bits: per pass on edge values (segment
and cell edges and either side, NaN, infinities, subnormals), and per row on
whole conversions; every picture-level test, the Chrome-pixel ones included,
runs both ways. A seventeen-row mutation table (`gui/video/yuv/mutate.py`)
shows each typical vector slip is caught.

**Alternatives.**

| | For | Against |
|---|---|---|
| Bit-identical (chosen) | one answer per pixel on every machine: a picture or screenshot looks the same on a PC with AVX2 and one without; the scalar code, held to Chrome's pixels, vouches for the vector code; any divergence is a bug a test sees | no FMA; the tone map's curved middle stays a pixel at a time (1.7x rather than ~3x where much of a picture is in it) |
| Close (FMA, a vector `exp2f`, free reordering) | more speed on bright pictures (not measured; the curved middle is what it would gain most on) | pixels one apart between machines; the vector code needs its own reference tests against Chrome, and a slip that moves pixels by one hides among the expected differences |

**Measured** (`bench_avx2_against_scalar`, i7-8700K, the two ways in turn):
steps 2 to 5 run 2.3x faster for PQ at MaxCLL 1000, 1.7x at 4000 (more light
in the curved middle), 3.6x with no tone map, 2.1x for HLG.

**Revisit when** the remaining speed matters (4K HDR at 60 frames a second
on a slow machine): a vector `exp2f` that is libm's to the bit is possible
(libm's is a table and a polynomial in double precision) and would lift the
curved middle without giving up identity -- `roadmap.md`, "HDR's speed, the
rest".
