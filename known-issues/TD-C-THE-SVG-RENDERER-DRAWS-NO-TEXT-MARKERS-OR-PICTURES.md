## TD-C-THE-SVG-RENDERER-DRAWS-NO-TEXT-MARKERS-OR-PICTURES (lane C, 2026-10-05)

**Status:** OPEN -- markers done 2026-10-05 (`gui/toolkit/src/svg/marker.rs`, part 1
below) and pictures in `data:` URLs the same day (`svg/image.rs`, part 3 --
the decoder was already linked, through `osfont`'s use of `imagecodec`).
Text drawn 2026-10-16 (2843a1b64, `svg/text.rs`, part 2): `<text>` and
`<tspan>`, as the outlines of their glyphs. What remains is the rest of part
2, which `svg/text.rs`'s "What is not" lists: `<textPath>`, `rotate`, a list
giving each character its own `x`/`dx`, `letter-spacing` and `word-spacing`,
italic, baselines other than the alphabetic, and text inside a `<clipPath>`.
Not followed, by design: an `<image>` naming another file or a URL, and an
SVG picture inside an SVG.

**In short:** the toolkit's SVG renderer, which draws every icon theme's
pictures, thumbnails and the image viewer's SVG files, now draws everything
SVG paints with -- shapes, gradients, patterns, clips, masks, filters,
markers, embedded pictures, and text on a line -- except text along a path
(`<textPath>`), whose characters it leaves out without a word, and a few
text properties (above), which it ignores: such text is drawn upright, on
the alphabetic baseline, evenly spaced. (This entry first listed text,
markers and pictures as all missing.)

**Where:** `gui/toolkit/src/svg/text.rs`: `gather` and `pieces_in` walk a
`<text>`'s own characters and its `<tspan>`s', and pass over any other
child, `<textPath>` included. (When this was filed: `svg.rs`'s `NOT_DRAWN`
listed `marker`, `build_node` built `text` and `image` as empty groups, and
`svg/filter.rs` built a file `feImage` as a transparent result.)

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
2. **Text** (done but for the list in the status) -- through lane F's `osfont`, which the toolkit already draws
   its own text with (`text::draw_into`): its shaping, and each glyph's
   outline (`sfnt`'s `outline`) filled and stroked as a path, so gradients,
   clips and filters apply to it as to any shape. `x`, `y`, `dx`, `dy`,
   `rotate`, `text-anchor`, `font-*` and `<tspan>` first; `<textPath>` after.
3. **Pictures** -- needs a decoder: PNG and JPEG at least, which the toolkit
   does not link today. Lane F's `gui/imagecodec` is the tree's; using it
   from the toolkit is a dependency to ask lane F about, not to take.

**If never fixed:** drawings that use them stay incomplete, silently. Nothing
else is affected.
