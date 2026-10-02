### [F] Glyph outlines were not HarfBuzz's to the bit -- at variable instances, and at the default in 133 of 328 host fonts -- 2026-09-26 -- **FIXED 2026-09-26**

**Status:** FIXED 2026-09-26 (lane F) -- `gui/font/src/glyf.rs` (new) builds a
`glyf` glyph as HarfBuzz 14.3.0 builds it, points first, and
`gvar::Gvar::apply` is HarfBuzz's `apply_deltas_to_points` (design-decisions
§1329).

**In short (as found):** the outlines this crate draws are meant to be
HarfBuzz's exactly, which is what lets `tools/outline_oracle.py` compare them
with `==` and call any difference a bug. For TrueType-outline fonts they were
not. At a variable font's in-between weights a point could sit a few
millionths of a unit from HarfBuzz's; and at the plain default instance, 133
of the 328 `.ttf` files on this host drew some glyphs differently -- a
contour started at another point, a line of no length missing, a scaled
component's point off in the last bit (`simsunb.ttf`: 8,820 of 12,091 glyphs
sampled; Arial, Times, Segoe UI, DejaVu: a handful each). Nothing visible on
screen; but as a reference test the oracle could not be used on these fonts.

**What differed.**

| | Before | HarfBuzz (now) |
|---|---|---|
| Order of work | each glyph's path built, then shifted; a component's path transformed | the whole glyph's *points* gathered, varied and shifted; only then the path |
| Halfway points between two controls | made up before the shift and the component's transform | after both |
| A component's transform | fused multiply-add | each product rounded, then the sum |
| A contour that starts off the curve | drawn from its first on-curve point | from its last point |
| The end of a contour | a line back to the start only if needed | always a line back to its first on-curve point, even of no length |
| A component placed by matching points | not moved at all | moved so that the two points meet |
| A `gvar` tuple's scalar | `f32` | `f64`, then `f32`; a shared tuple's as HarfBuzz's draw cache hands it back |
| Interpolated (IUP) deltas | from unscaled deltas, then scaled | from the scaled deltas |
| Summing the tuples | all summed, then added | straight into the points when no tuple names its own; otherwise in batches |
| Packed data | point list longer than the glyph refused; numbers wrapped at 65,536; control `0xC0` read as zeros | as HarfBuzz: kept, summed in 32 bits, 32-bit deltas |

**Checked:** `tools/outline_oracle.py` -- now reporting where two paths first
part, and drawing each glyph twice so that HarfBuzz's scalar cache is warm --
over every `.ttf` on the host at its default instance (328 fonts,
200,945 glyph-instances, every fifth glyph): every path and box agrees but
the colour-glyph boxes of the entry below. And over the variable fonts at
their named instances (7 fonts, 18,615 glyph-instances):
every path and box agrees. The hint fixture pins HarfBuzz's drawing of its variable
TrueType face at weight 610 and at 401, where the scalar is below 2^-6 and
the cache rounds it (`glyf::tests`).

**HarfBuzz's own behaviours, reproduced on purpose** (§1329): a cubic
contour's leftover first control point reaching the next contour
(`path_builder_t` does not clear `first_offcurve2`); a point-matched
component's parent point counted from the start of the whole glyph; the
cycle detector noticing a cyclic composite a level late. *Not* reproduced:
HarfBuzz's draw cache can hand a tuple with an intermediate region the cached
0 or 1 of a plain tuple with the same peak, a 0 where the region past its
peak should give more -- which depends on the glyphs drawn before, so the
crate weighs the region. The leak and the cache's answer look like HarfBuzz
bugs, worth reporting upstream; the point counting may be one too (FreeType
counts from the start of the composite being built, which differs only for a
nested one).
