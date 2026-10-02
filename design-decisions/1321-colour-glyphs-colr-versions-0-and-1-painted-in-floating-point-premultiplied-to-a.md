## 1321. Colour glyphs: `COLR` versions 0 and 1 painted in floating point, premultiplied, to a picture that is cached per glyph and drawn beside the coverage masks

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** emoji came out as black silhouettes -- or as nothing -- because
the glyph drawer only knew coverage masks tinted with the text colour, and an
emoji font stores its pictures as recipes: layers of outlines, each in its own
colour, or a graph of gradients, transforms and blend modes. Now a glyph whose
face has a recipe for it is painted into a small colour picture, cached like
the masks, and drawn as a picture by `draw_text` and by the compositor. Noto
Color Emoji's vector build draws as Chrome draws it: measured against Edge on
the same file, the differences are the edges a pixel-snapped glyph position
moves, nothing inside a glyph.

**Decision.**

* **One renderer for both versions** (`osfont::colr::render`): version 0's
  layers are painted as solid fills through the same path as version 1's
  `PaintGlyph`, so there is one rasterizer, one blender and one set of limits.
* **Floating point, premultiplied RGBA, gradients interpolated
  premultiplied.** There is no reference rendering to match bit for bit, as
  there is for the image codecs -- every engine rasterizes outlines its own
  way -- so the arithmetic follows the specification's definitions rather
  than any one engine's integer shortcuts, and the check is against Skia's
  output (Edge, `target/colr_compare.py`) rather than to the bit.
* **fontTools' conventions where the specification leaves room**, since the
  fonts are built with fontTools: skew by `tan(-x)` and `tan(y)`, rotation
  counter-clockwise, a transform's paint drawn in the space it transforms
  (innermost first), and sweep angles stored less half a turn.
* **The canvas is the glyph's clip box, or the union of its outlines' boxes**,
  snapped to whole pixels with a 1/256-pixel allowance, so float error in a
  transform does not cost a column of empty pixels.
* **Cost:** a layer is rasterized over the pixels its box touches; a fill
  under a `PaintGlyph` (through transforms) is evaluated only where the
  outline covers; only a `PaintGlyph` over a more complicated graph, and a
  composite, get a scratch canvas. About a millisecond per 64-px emoji the
  first time, then a cached blit.
* **Hostile fonts cost a bounded amount:** depth 64, 20 000 paint visits,
  64 touches per canvas pixel, four canvases alive at once, a 1024x1024 cap
  on the glyph (bigger draws the outline instead). Every offset is checked; a
  mutation fuzz of every paint format is a unit test.
* **The text colour is not an ingredient of the cache key unless the glyph
  used it** (`ColourImage::uses_foreground`): palette entry `0xFFFF` is the
  text colour, a glyph that paints with it is drawn again for another colour,
  and one that does not -- most emoji -- is drawn once for every colour of
  text. The text colour's *alpha* is applied to the finished picture as an
  opacity, not passed in, or a half-transparent text colour would be applied
  twice to what used it.
* **A separate cache** in `ScaledFont`, with a pixel ceiling (16 MiB) as well
  as a count, since a colour glyph weighs four bytes a pixel to a mask's one.
* **The compositor gets a `draw_colour_glyph` primitive** on its
  `RenderTarget` seam, beside `draw_glyph`: a GPU backend keeps these in a
  colour atlas beside the coverage one. The software backend unpremultiplies
  each pixel into its existing `blend_pixel`, so a colour glyph goes through
  the same clip, frame clip and window opacity as everything else.

**Alternatives.**

| | For | Against |
|---|---|---|
| Fixed-point arithmetic, as for the codecs | deterministic across machines | no reference to be exact *to*; costs clarity for nothing measurable |
| Render every layer to its own canvas and composite | the specification's model, literally | a scratch canvas per layer, where most layers are one fill in one outline |
| Draw the text colour's alpha into the picture | one fewer parameter | applied twice where the glyph used the text colour; a cache entry per alpha |
| Rasterize the picture at draw time, no cache | nothing to invalidate | a millisecond per emoji per frame |

**Variations** (added the same day): a variable colour font's paints, colour
stops and clip boxes move with its axes, each field by the delta its
`varIndexBase` names -- through the `DeltaSetIndexMap` if there is one -- in
the field's own units, read through the same `varstore` as `HVAR`'s.

**Not modelled** (`known-issues.md` [F] 2026-09-26): palettes other than the
first; a `PaintColrGlyph`'s clip box. Colour *bitmaps* (`CBDT`, `sbix`) are
§1322.

**How to reverse.** `ScaledFont::colour_glyph` and `SystemFont::glyph_image`
are the only ways in; a face without `COLR` never reaches the renderer, and
without them every glyph is drawn from its mask as before.
