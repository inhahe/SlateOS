## TD-C-THE-SVG-RENDERER-DRAWS-NO-TEXT-MARKERS-OR-PICTURES (lane C, 2026-10-05)

**Status:** OPEN

**In short:** the toolkit's SVG renderer, which draws every icon theme's
pictures, thumbnails and the image viewer's SVG files, now draws everything
SVG paints with -- shapes, gradients, patterns, clips, masks, filters --
except three things, which it leaves out without a word: text (`<text>`,
`<tspan>`, `<textPath>`), markers (the arrowheads and dots `marker-start`,
`marker-mid` and `marker-end` put on a line), and pictures (`<image>`, and an
`feImage` naming a picture file rather than an element). An icon or drawing
that uses them shows everything else, with those parts missing.

**Where:** `gui/toolkit/src/svg.rs`: `NOT_DRAWN` lists `marker`; `build_node`
builds an unknown element -- `text`, `image` -- as an empty group;
`svg/filter.rs` builds an `feImage` with a file `href` as a transparent
result.

**Why it matters less for icons than it sounds.** Icon themes convert their
text to paths before they ship (Breeze, Adwaita and Papirus all do), and
markers belong to diagrams, not icons. Pictures are rarer still: an SVG icon
that embeds a PNG is usually a PNG in disguise. The image viewer is where all
three show, since it opens whatever SVG a user has.

**The proper fix, one part at a time:**

1. **Markers** -- self-contained: each vertex of a path, line, polyline or
   polygon takes the `<marker>` its `marker-*` properties name, drawn in its
   own viewport (`markerWidth`, `markerHeight`, `viewBox`, `refX`, `refY`)
   turned by `orient` (`auto`, `auto-start-reverse` or an angle) and scaled by
   `markerUnits` (`strokeWidth` by default). The viewport placement is
   `viewport_placement`'s, and a marker is content drawn as a `<use>` is.
2. **Text** -- through lane F's `osfont`, which the toolkit already draws
   its own text with (`text::draw_into`): its shaping, and each glyph's
   outline (`sfnt`'s `outline`) filled and stroked as a path, so gradients,
   clips and filters apply to it as to any shape. `x`, `y`, `dx`, `dy`,
   `rotate`, `text-anchor`, `font-*` and `<tspan>` first; `<textPath>` after.
3. **Pictures** -- needs a decoder: PNG and JPEG at least, which the toolkit
   does not link today. Lane F's `gui/imagecodec` is the tree's; using it
   from the toolkit is a dependency to ask lane F about, not to take.

**If never fixed:** drawings that use them stay incomplete, silently. Nothing
else is affected.
