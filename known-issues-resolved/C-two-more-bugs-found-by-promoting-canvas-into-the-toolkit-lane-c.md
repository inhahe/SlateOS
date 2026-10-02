## Two more bugs found by promoting `Canvas` into the toolkit (lane C)

**Status: FIXED 2026-08-16** (lane C). Both were found by writing tests for
`guitk::canvas::Canvas` — the shared type extracted so `apps/paint` need not
write a pixel buffer a third time — and one of them was *live in already-pushed
code*: it shipped in `b10a4fc1f` as `explorer`'s `box_filter_downscale`, in the
same commit that fixed the six bugs above. That is the argument for the
extraction restated: the local copy had no test that would have caught it, and
writing the shared type's tests caught it within the hour.

**1. Box-filter downscaling double-weighted every boundary pixel.** Mapping a
destination pixel `d` back to a source span used `floor` for the lower end and
`div_ceil` for the upper — the obvious way to write "round outwards so nothing
is missed". Rounding outwards at *both* ends makes adjacent spans **overlap**
wherever the ratio is not an integer, and a source pixel counted by two
destination pixels is averaged into both. Reducing 10 pixels to 3 covers them
`[1,1,1,2,1,1,2,1,1,1]`. The result is not a crash or a visibly wrong image; it
is a thumbnail that is subtly, unevenly soft, which is why no one noticed.

The fix floors both ends, so the spans tile the source exactly once, with
`.max(lo + 1)` to keep a span from being empty when only one axis shrinks (a
2 × 100 image asked for 8 × 3 enlarges the width while reducing the height).
Pinned by `span_covers_every_source_pixel_exactly_once`, which asserts the
coverage histogram is all-ones for a spread of ratios, and by
`a_span_is_never_empty_even_when_enlarging_one_axis`.

**2. A canvas whose dimensions multiply within `usize` can still abort the
process.** `Canvas::pixel_count` originally checked only `width * height`
against `usize` overflow. On 64-bit that check passes for `u32::MAX ×
u32::MAX` — about 1.8 × 10^19, which fits — and then `Vec` computes
`n * size_of::<Color>()` bytes, exceeds the `isize::MAX` allocation limit and
**aborts**, not panics, so no caller can catch it. Constructors are supposed to
return `None` for dimensions they cannot address. Now the byte count is checked
too, and `dimensions_that_cannot_be_addressed_collapse_to_empty` covers it.
