## TD-C-THE-SVG-RENDERER-DREW-STROKES-AT-THE-WRONG-WIDTH-AND-CURVES-AT-HALF-STRENGTH (lane C, 2026-09-26) -- FIXED the same day

**Status:** FIXED 2026-09-26 -- `gui/toolkit/src/svg.rs`: a fill, and a stroke,
is one coverage pass; strokes are outlined with joins and caps and scale with
the drawing.

**In short:** the toolkit's SVG renderer, which draws the desktop's icons and
the thumbnails of SVG files, drew every line as many screen pixels wide as the
file said in its own units -- two pixels at every size, heavy at 16 and a
hairline at 64 -- and drew curves as dozens of small pieces, each blended on
its own, so a circle's outline came out at half strength and a see-through line
darkened at every joint. It also filled each part of a shape separately (the
hole of an "O" was filled in), cut arcs into eight pieces a turn whatever their
size, read `rx` without `ry` as square corners, and dropped the first segment
drawn after a closepath.

**How it was found:** drawing the shell's pictograms (design-decisions §881),
a sun at 16 pixels had no pixel even three-quarters covered.

**The fix.** Fills and strokes go through one scanline pass per shape with a
winding count over every edge of every subpath (nonzero or even-odd, as
`fill-rule` says), blending each pixel once. A stroke is the union of a quad per
segment, a join per corner (`stroke-linejoin`, with `stroke-miterlimit`) and a
cap per open end (`stroke-linecap`), all wound alike and filled nonzero. Its
width is scaled by the transform. Circles and arcs are cut to a tenth of a pixel
at the size they are drawn. Twelve tests, fourteen mutations, all killed.

**Also gone:** `SvgDocument::render_commands`, which approximated every path
fill by its bounding box and which nothing called.
