## 1323. Per-pixel code avoids `f32::mul_add`, `round`, `floor` and `ceil`: on the baseline target each is a C-library call

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** four ordinary-looking float methods are not instructions on
this workspace's x86_64 target but calls into the C library, because the
target does not assume the CPU features that make them instructions. In code
that runs once a pixel they were most of the cost: colour emoji painted at
half speed and the compositor's backdrop blur spent 48 of its 62 ms a
megapixel in them. Per-pixel loops in lane F now avoid them; code that runs
once a row, a glyph or an event keeps them, where they cost nothing.

**The facts** (`rustc -O --emit asm --target x86_64-pc-windows-gnu`, which
has the same baseline as the SlateOS target):

| Method | Compiles to | Why |
|---|---|---|
| `f32::mul_add` | `jmp fmaf` | FMA is not in the baseline; the single rounding it promises needs software |
| `f32::round` | `jmp roundf` | `roundss` is SSE4.1 |
| `f32::floor`, `ceil` | `jmp floorf`, `ceilf` | likewise |

**Decision.** In a loop over pixels:

* a multiply-add is written `a * b + c` (`osfont`'s `raster::mad` names it,
  with the reason) -- two instructions, and the lost half-ulp is invisible in
  a coverage value or a colour;
* rounding a non-negative value to an integer is `(v + 0.5) as u32` after the
  clamp, the same result as `round()` for every value at or above zero bar
  exact float ties;
* `floor` of a value known non-negative is `as` truncation.

Measured: colour glyph painting (`colr.rs`, `bitmap.rs`) 2x faster; the
backdrop blur's saturation pass 61.6 -> 13.4 ms a megapixel; glyph masks
byte-identical before and after.

**Alternatives.**

| | For | Against |
|---|---|---|
| Raise the baseline (x86-64-v2 for SSE4.1, v3 for FMA) | every method becomes one instruction everywhere | drops CPUs the OS otherwise runs on; lane A's and D's target specs, not lane F's; an operator decision |
| Runtime dispatch (`is_x86_feature_detected!`) | fastest where available | two code paths per loop for a saving the plain arithmetic already gets |
| Leave them | exact single rounding | a pixel has no use for it, and it halved throughput |

**How to reverse.** Nothing depends on the unfused forms; a raised baseline
would make `mul_add` and `round` single instructions and these rewrites
merely equivalent.
