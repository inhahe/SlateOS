### [F] A scaled picture's translucent edges come out darker than they are -- 2026-09-25

**Status:** HALF FIXED 2026-09-25 — `gui/imagecodec`'s `BoxFilter` (the PNG
and GIF scaled decodes) now weights colour by alpha. What remains is lane C's:
the thumbnailer's second shrink, `Canvas::box_downscale` through
`Color::mean`, still averages the channels separately, so a thumbnail's rim is
halved rather than gone -- `requests/f-c-average-translucent-pixels-by-their-alpha.md`.

**In short:** when a picture with see-through parts -- an icon, a transparent
PNG, a GIF -- is shrunk for a thumbnail, the pixels along its see-through edges
come out darker than the picture itself, as if outlined in grey. The shrinking
averages each group of pixels, and it averages a fully transparent pixel's
colour in with the visible ones even though that colour is never seen (it is
usually black).

**Where.** `gui/imagecodec/src/scale.rs`, `BoxFilter::finish` (the PNG and GIF
scaled paths), and `gui/thumbs`' `box_filter_downscale`, which the former
matches on purpose so that a scaled decode and a decode-then-scale agree.
Both sum alpha, red, green and blue separately and divide each by the count.

**Why it is wrong.** Straight (not premultiplied) colour has to be averaged
weighted by alpha: a half-covered cell of red over transparent black is red
at half opacity, but averaging the channels separately makes it dark red at
half opacity. The compositor then blends that dark red over whatever is behind
the thumbnail.

**The proper fix.** Weight each pixel's colour by its alpha when summing, and
divide the colour sums by the alpha sum (the cell's alpha stays the plain
mean) -- in both places at once, so that the rule stays one rule. Test it with
an opaque disc on a transparent field: every edge pixel of the thumbnail should
have the disc's colour at partial alpha.

**How to see it.** Thumbnail a GIF or PNG with an opaque coloured shape on a
transparent background over a light background: the shape has a dark rim.
