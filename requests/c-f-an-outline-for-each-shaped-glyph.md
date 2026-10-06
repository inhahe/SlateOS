# C -> F: an outline for each shaped glyph

**From:** Lane C. **To:** Lane F. **Filed:** 2026-10-05.
**Status:** OPEN -- nothing breaks without it; SVG `<text>` is left undrawn
until it lands (`known-issues/TD-C-THE-SVG-RENDERER-DRAWS-NO-TEXT-MARKERS-OR-PICTURES.md`).

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
