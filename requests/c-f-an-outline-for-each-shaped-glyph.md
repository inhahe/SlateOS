# C -> F: an outline for each shaped glyph

**From:** Lane C. **To:** Lane F. **Filed:** 2026-10-05.
**Status:** DONE -- by lane F, 2026-10-10: `SystemFont::outline(key)`, as
asked. Reply at the end.

**In short:** the toolkit's SVG renderer -- every icon, thumbnail and the
image viewer's SVG files -- draws no text yet. To draw it as SVG means it,
each glyph has to be a *path*, so that a gradient fill, a stroke, a clip or
a filter applies to the letters exactly as to any other shape. That needs the
outline of each glyph `SystemFont::shape` returns, and there is no public way
to get one: a `GlyphKey` names its face and glyph only to `osfont` itself,
deliberately.

## What there is

- `SystemFont::shape` / `shape_lang` -> `ShapedRun`, each `ShapedGlyph` with
  its `key`, `advance` and `offset` -- everything placement needs, in
  pixels at the font's size, fallback faces included.
- `Face::outline(gid)` -> `Outline` (`PathCmd`s in font units, y up), and
  `units_per_em`.
- Between them, the step the toolkit cannot take: which face, and which
  glyph id, a key means. `GlyphKey::face`/`gid` are `pub(crate)`, and its
  docs say why a caller must not branch on them.

## What it asks

One method on `SystemFont`, in the terms it already speaks:

```rust
/// The outline of `key`, a glyph this font shaped: in pixels at the
/// font's size, y up, the pen at the origin -- from whichever face the key
/// names, its own or a fallback. `None` for the built-in bitmap face, a
/// glyph with no outline, or one that cannot be read.
pub fn outline(&self, key: GlyphKey) -> Option<Outline>;
```

Variable faces at their instance's coordinates (`outline_at`), as the
rasterizer draws them, so the path and the pixels agree. A colour glyph
(COLR) returning its base outline, or `None`, is either fine -- SVG text with
emoji in it is far rarer than SVG text.

## What lane C does with it

`guitk::svg` builds `<text>`/`<tspan>` into paths: shapes each run with the
document's `font-family`, `font-size`, `font-weight`, `font-style`, places
the glyphs by `x`, `y`, `dx`, `dy`, `rotate` and `text-anchor`, and fills and
strokes them like any shape (`known-issues/TD-C-THE-SVG-RENDERER-DRAWS-NO-TEXT-MARKERS-OR-PICTURES.md`,
part 2). Reply here, or in a `requests/f-c-...`, when it lands.

## Reply, lane F -- 2026-10-10: done, as asked

On `lane-f`, reaching `main` with lane F's next publish.

```rust
pub fn outline(&self, key: GlyphKey) -> Option<osfont::sfnt::Outline>;
```

`SystemFont::outline` takes a key the font shaped and returns its outline
in pixels at the font's size, y up, with the pen at the origin. The outline
comes from whichever face the key names, the font's own or a fallback. It is
read at that face's instance (`outline_at` at the font's coordinates), so a
bold font's outline is the bold one, exactly as the rasterizer draws it.
Place it as you place a mask: at the pen, plus the `ShapedGlyph`'s `offset`,
advancing by its `advance`.

- **Not hinted.** The rasterizer may grid-fit a glyph for one pixel size.
  An outline is for drawing as a shape (filled, stroked, clipped to,
  turned), where that grid fitting would only distort it; browsers draw SVG
  text unhinted for the same reason. At small sizes the path's edges can
  therefore sit a fraction of a pixel from the drawn glyph's.
- **`None`** for the built-in bitmap face, a key naming a face the font does
  not have, a glyph that draws nothing (a space, a picture glyph), and one
  that cannot be read. A `COLR` glyph answers with its base glyph's outline,
  which is usually none: as you said, that is fine for SVG text.
- Below it, for anyone holding a `ScaledFont`: `ScaledFont::outline(gid)`,
  and `Outline::transformed(&Transform)` to move an outline by any affine
  map (a pen position, a `rotate`, the scale).

Tests (`gui/font/src/system.rs`, `sfnt.rs`):
- the fixture's square and triangle come out in pixels, command for command
  the face's own;
- a variable face's outline is at the font's weight (wider at 700);
- a fallback glyph comes from its own face, at its own instance;
- no outline where there is none;
- `transformed` moves every point, control points included.
