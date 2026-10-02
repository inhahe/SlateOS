### [F] Lossy WebP decodes at about half libwebp's speed -- 2026-09-25

**Status:** OPEN — lane F's; tech debt, not a bug. Mostly paid: the steps
below are done, and the gap is now 5 to 12 per cent rather than a half.

**In short:** a large lossy WebP -- the common kind, a photograph -- took
about twice as long to open here as in a browser. Measured in the decoding
thread's CPU cycles (wall time is useless on a machine shared with QEMU and
five other sessions), it now takes 3.36 billion cycles for a 4000x5333
picture against libwebp's 2.99 (it was 5.03), and 0.31 for 2000x1500 against
0.30 (was 0.57). The pixels are libwebp's to the bit, corrupt files
included; only the speed is a little behind. Lossy animations inherit it.

**Where.** `gui/imagecodec/src/webp/lossy/`. Before the last step the time
split roughly: coefficient tokens 36%, loop filter 25%, conversion to RGB
21%, prediction and inverse transform 18%; the conversion has since shrunk
the most.

**Done** (each measured on its own, each held to the fixtures and to 5,856
corrupted and truncated files decoded exactly as libwebp decodes them):
1. The conversion a row at a time: each chroma column blended down once per
   row, each pixel then two lookups in a row buffer (was four bounds-checked
   lookups per channel per pixel).
2. The loop filter an edge at a time: sixteen segments' taps loaded as eight
   rows of 16-bit lanes and filtered lane-wise with selects instead of
   branches, which SSE2 can do (32-bit lanes were slower than the scalar
   code: SSE2 has no 32-bit min, max or abs).
3. The token loop's probabilities banded by position, one lookup a
   coefficient (libwebp's `bands_ptr`), and the boolean decoder's refill one
   eight-byte load.
4. The conversion itself in libwebp's SSE2 formulation -- 16-bit
   multiply-highs, saturating unsigned blue -- held equal to the scalar one
   on all 2^24 inputs by a test, over whole rows the compiler vectorises; and
   the horizontal chroma upsampling over three shifted views of the row, the
   two end columns apart.

**The proper fix, the rest:** libwebp's remaining edge is SIMD it writes by
hand for the inverse transform and the predictors. The Rust equivalent is
the shape the filter and the conversion now have: fixed-size 16-bit lane
arrays the compiler can vectorise.

**How to see it.** `target/perfbench` (a scratch harness: the working crate
against a snapshot copy, interleaved, the minimum of N runs in thread
cycles), and libwebp's cycles from Pillow's `load()` measured the same way
with `QueryThreadCycleTime`.
