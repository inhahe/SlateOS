### [F] Text is never hinted: the `hinting` font setting changes nothing -- 2026-09-26

**Status: FIXED 2026-09-26** (lane F) — the setting now switches on a port of
FreeType 2.13.2's auto-hinter in its light mode (`gui/font/src/hint/`,
design-decisions §1325), which the compositor turns on from the settings. It
agrees with FreeType point for point on every Latin, Greek, Cyrillic, Arabic,
Hebrew, Armenian and Devanagari glyph of six fonts checked at eleven sizes
(`gui/font/tools/hint_oracle.py`), and on every glyph of two CFF fonts with
fractional coordinates, both coordinates exact -- small capitals, superscripts
and the other feature forms included, each sorted into FreeType's style for
it. The gap this first left -- ideographs and the fallback style unhinted --
is closed too (the entry below). (Composites with borrowed metrics, which sat a hair off
horizontally, were a placement bug rather than a hinting one, and are fixed:
see the entry below.)

**In short (as filed):** the appearance settings have a "hinting" switch, on by default,
and nothing reads it: glyph outlines are rasterized exactly as designed, never
nudged onto the pixel grid. At small sizes that leaves stems and horizontal
bars straddling two pixels, so text looks softer than it does in programs that
hint. (Smoothing and the subpixel order, the setting's two neighbours, are
honoured: design-decisions §1324.)

**Where:** `gui/font/src/raster.rs` (the rasterizer takes outlines as they
come), `gui/compositor/src/lib.rs` `font_rendering` (which maps the other two
settings and says why not this one).

**The proper fix, and the choice in it:**
1. **A light autohinter** -- snap each glyph's horizontal features (baseline,
   x-height, cap height, horizontal stems) to whole pixels vertically, leave
   the horizontal axis alone. This is what FreeType's "light" mode and
   DirectWrite's ClearType natural mode do, needs no font data, and works on
   CFF and TrueType alike. The better first step.
2. **The TrueType instruction interpreter** -- run each font's own hinting
   program. Exactly what the font's designer intended, but a large virtual
   machine (FreeType's is some eight thousand lines of C), and useless for CFF
   fonts and for the many modern fonts shipped unhinted.

**How to see it.** `target/fontcheck modes` draws a line in each rendering
mode; compare a small size against the same text in Chrome.
