## 1322. Colour bitmap glyphs: `CBDT` and `sbix` pictures decoded by imagecodec, taken from the strike at or above the size, resampled by area

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** some emoji fonts store each emoji as a finished picture at one
or two fixed sizes rather than as a recipe -- Noto Color Emoji's bitmap build,
Apple's emoji font. Face fallback already chose such a face for emoji, and
then had nothing to draw: the emoji came out blank, and a face with no
outlines at all (Noto's bitmap build has none) was refused outright. Now the
picture is decoded, scaled to the text size, and drawn through the same cache
and the same paths as a `COLR` glyph (§1321).

**Decision.**

* **A face of pictures parses** (`Outlines::Pictures`): no `glyf` or `CFF` is
  accepted from a face with `CBLC`+`CBDT` or `sbix`, every outline empty.
  A face with neither outlines nor pictures is still refused.
* **The strike at or above the size asked for, else the biggest**:
  shrinking a picture loses less than enlarging one. Noto has one strike, so
  in practice this is "Noto's 109-pixel picture, shrunk".
* **Resampled here, by area when shrinking and bilinearly when enlarging**,
  in premultiplied colour so a transparent edge does not darken -- not with
  imagecodec's `decode_scaled`, whose box filter assigns each source pixel
  to one output cell (right for thumbnails, blocky for a 20-pixel emoji) and
  which does not enlarge.
* **Placed from the strike's metrics, scaled, rounded to a whole pixel**:
  `CBDT`'s bearings name the top-left corner; `sbix`'s origin offset the
  bottom-left, as FreeType reads it.
* **imagecodec is a dependency of osfont.** Both are lane F's, both `no_std`
  + `alloc` in intent, and it is the one PNG decoder in the tree; the
  alternative, handing encoded bytes up for every caller to decode, puts a
  decoder in the toolkit and another in the compositor.
* **Everything a `CBDT` can say is read** -- index formats 1 to 5, image
  formats 17, 18 and 19 -- and in `sbix` PNG, JPEG and TIFF pictures and one
  `dupe` hop. The monochrome and grey formats (1 to 9) are not colour and
  are left to the outline.
* **A recipe wins over a picture** where a face has both, since it is drawn at
  the size rather than resampled.

**How to reverse.** `bitmap::render` is called from one place,
`ScaledFont::colour_glyph`; `Outlines::Pictures` is reached only from a face
with bitmap tables.
