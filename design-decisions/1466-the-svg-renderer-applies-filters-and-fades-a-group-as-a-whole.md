## 1466. The SVG renderer applies filters, and fades a group as a whole

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the toolkit's SVG renderer -- which draws every icon theme's
pictures, thumbnails and the image viewer's SVGs -- ignored the `filter`
property, so a soft shadow under an icon, a blur, a glow or a recolouring
was simply missing. It now applies every filter Filter Effects 1 defines,
and CSS's shorthand filters (`blur()`, `drop-shadow()`, `grayscale()` and
the rest). Doing that needed what the renderer also lacked for opacity: a
way to draw an element on a surface of its own and lay it down afterwards.
With it, a half-transparent group of overlapping shapes is now see-through
evenly, instead of darker wherever its shapes overlap.

**Where:** `gui/toolkit/src/svg/effects.rs` (each primitive's arithmetic),
`gui/toolkit/src/svg/filter.rs` (reading `<filter>` and `filter`, regions,
units, evaluation), `gui/toolkit/src/svg.rs` (`render_layered`,
`render_faded`, `render_filtered`, `lay`).

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **A layer only where an opacity has something of its own to overlap** -- a group, a `<use>`, a shape with both a stroke and a fill | a layer for every element with an opacity below one | A shape with one paint, faded, is exactly that paint faded: no layer, no cost, and the same pixels as before. Most faded elements in icons are single paths. | Two code paths that must agree; a test holds that they do. |
| **A layer sized to what the element can reach** -- its shapes, grown by how far their strokes can (a miter at its limit) | a layer the size of the drawing | A drawing's scratch budget is sixteen surfaces' worth of pixels; full-size layers would spend it after sixteen faded groups. Small layers let a drawing have hundreds. | Working out the reach is a walk of the element's subtree; it is bounded like every other walk. |
| **Past the budget, a faded group fades each part instead** | drawing nothing | The overlaps show, as they did before layers -- a smaller error than a missing group. | A pathological drawing renders slightly differently from a normal one. |
| **Past the budget, a filtered element is not drawn** | drawing it unfiltered | An unfiltered "shadow" shape is a hard dark blot where a soft shadow was meant; leaving it out is the smaller error, and the one masks already make. | A pathological drawing loses elements. |
| **A region covering more than 2048 x 2048 pixels is filtered at a lower resolution and scaled up** | filtering at full resolution whatever the size | Bounds the memory one filter takes (its intermediate images are four floats a pixel) and the time, as Firefox does. | A huge blurred region in a huge drawing is a little softer than asked. |
| **Errors follow Filter Effects 1, not SVG 1.1** -- a negative deviation or radius disables the primitive (its input passes through), a malformed convolution passes through, a `filter` naming no `<filter>` filters nothing | SVG 1.1's "the element is not rendered" | It is the current specification and what browsers do; an icon with a slip in its filter still shows. | Where Filter Effects 1 itself says "not rendered" -- an empty `<filter>`, a box with no area under bounding-box units -- the element is still left out, as browsers do. |
| **Filter functions work in sRGB**; `<filter>` primitives in their `color-interpolation-filters`, linear light by default | linear light for both | The specification defines the functions by sRGB primitives, and it is what browsers draw. | The two give slightly different blurs of the same colours. |
| **`drop-shadow()`'s blur is a radius, twice the deviation** | the length as the deviation | The specification says its values are read "as for `box-shadow`", whose blur radius is twice the Gaussian's deviation. | A shadow ported from an `feDropShadow` with the same number is half as soft. |
| **Offsets turn with the element; deviations and radii scale by each axis** | resampling a turned element into an upright one first | Exact for the turns and scales icons use; no resampling blur. | An unequal blur on a turned element is blurred along the screen's axes, not the element's. |

### What is not drawn

Text, which waits on lane F for glyph outlines
(`requests/c-f-an-outline-for-each-shaped-glyph.md`), and a picture named by
a file path or a URL rather than carried in a `data:` URL -- including an
`feImage` naming one -- which a drawing may not reach out for:
`known-issues/TD-C-THE-SVG-RENDERER-DRAWS-NO-TEXT-MARKERS-OR-PICTURES.md`.
(Markers and `data:` pictures followed the same day.) `BackgroundImage` and
`BackgroundAlpha`, which no browser draws either, are transparent.

### Added later the same day: filters' work is budgeted too

The scratch budget charged each primitive its region's area, which bounds
the surfaces a filter fills but not the work: turbulence of 32 octaves is
64 passes a pixel and a convolution up to 128, so a document chaining them
up to the scratch budget made one drawing a minute's work. The fuzz test
(`gui/toolkit/tests/svg_fuzz.rs`) found it on its first long run.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **A second budget, of work: each primitive charges its area times its cost in passes of the cheapest primitive** (`Kind::cost`: turbulence two a octave, a convolution one per eight kernel values, a blur three) -- 32 a pixel of the drawing, and at least 2^22 | charging expensive primitives more of the scratch budget | Memory and time are different limits and fail differently; one budget for both would make a large flood starve a small turbulence or the reverse. 32 passes a pixel is a dozen ordinary primitives over the whole drawing, or a full-size turbulence of sixteen octaves. | The costs are estimates, not measurements of each primitive; a primitive whose work grows past its estimate escapes. Past the budget a filtered element is not drawn, as past the scratch budget. |
