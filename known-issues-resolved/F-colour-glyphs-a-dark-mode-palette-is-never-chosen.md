### [F] Colour glyphs: a dark-mode palette is never chosen -- 2026-09-26 -- **FIXED 2026-09-26**

**Status:** FIXED 2026-09-26 (lane F) — both parts. `colr::ColourPalette`
chooses a palette as CSS's `font-palette` does (`palette_index`: the first
palette `CPAL` version 1 marks for a light or a dark background, the first
palette where none is marked), `render` paints with it, and it rides in
`Rendering`, so `ScaledFont` repaints its colour glyphs -- and only those --
when it changes. The compositor, which draws every process's text, sets it
from the theme: `Light` on a light theme, `Dark` on a dark one
(design-decisions §1327). A `PaintColrGlyph` now cuts the glyph it names to
that glyph's clip box, under the transform in force, and bounds its canvas by
it.

**In short (as filed):** emoji are drawn in colour whichever way their font stores them
-- a recipe (`COLR`, variable ones included) or pictures (`CBDT`, `sbix`) --
and match Chrome. What is not yet honoured is a font's *second* palette: the
dark-mode colours some colour fonts carry for text on a dark background.

**Where:** `gui/font/src/colr.rs` (`render` takes palette 0).

**What is missing, and the proper fix:**
1. **Palette choice**: `CPAL` version 1 marks palettes usable on light or
   dark backgrounds. A `palette` argument to `render`, chosen by the caller
   from the theme and made part of the colour cache's key.
2. **`PaintColrGlyph`'s clip box**: the referenced glyph's `ClipBox` should
   clip its graph; it is ignored (the outer canvas still bounds it). No font
   seen so far depends on it.

**How to see it.** `target/fontcheck` draws emoji lines from
`target/fonts/notoemoji__Noto-COLRv1.ttf` (drawn) and
`target/fonts/notocoloremoji__NotoColorEmoji-Regular.ttf` (blank).
