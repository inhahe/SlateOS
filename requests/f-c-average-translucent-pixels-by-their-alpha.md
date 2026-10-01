# F → C — Average translucent pixels by their alpha, so shrunken pictures keep their edges

**From:** Lane F (`gui/imagecodec`). **To:** Lane C (`gui/toolkit`, `gui/thumbs`).
**Filed:** 2026-09-25.
**Status:** DONE — lane C's half landed 2026-09-25 (1ef989906): `guitk::color::Color::mean`
weights each channel by alpha, as `Canvas::box_downscale` does, so the two agree about the
same picture.

**In short:** when a picture with see-through parts -- an icon, a transparent
PNG, a GIF sprite -- is shrunk for a thumbnail, its see-through edges come out
darker than the picture, as if outlined in grey. `imagecodec`'s scaled decode
no longer does this; the thumbnailer's second shrinking step still does, so
the rim is halved rather than gone. The fix is one averaging rule, in
`Color::mean` or `Canvas::box_downscale`.

## What is wrong

`gui/toolkit/src/canvas.rs`, `Canvas::box_downscale`, averages each region
with `Color::mean`, which (as `imagecodec` did until today) sums alpha, red,
green and blue separately and divides each by the count. A fully transparent
pixel's colour is never seen -- and is usually black -- but it is averaged in
all the same: half a cell of opaque red over transparent black becomes *dark*
red at half opacity, and the compositor blends that dark red over whatever the
thumbnail sits on.

## The rule

Weight each pixel's colour by its alpha: the region's alpha is the plain mean
of the alphas, and each colour channel is `sum(channel * alpha) / sum(alpha)`
-- or clear, if nothing in the region shows. For an opaque region this is the
same arithmetic as now and gives the same bytes, so every opaque thumbnail is
unchanged; only translucent edges change, and they change to right.

`gui/imagecodec/src/scale.rs`, `BoxFilter`, is the rule as `imagecodec` now
applies it, with its tests (`an_edge_over_transparency_keeps_its_colour_and_loses_only_opacity`,
`opaque_pictures_average_exactly_as_the_channels_do`). The two must agree, or
a scaled decode and a decode-then-scale disagree about the same picture --
which is the reason `imagecodec` gave for matching the old rule, and now the
reason to match the new one.

## Where

- `Color::mean`, if every caller of it is averaging pixels (then fixing it
  there fixes them all), or `Canvas::box_downscale` alone if some caller wants
  a plain channel mean.
- `gui/thumbs/src/lib.rs`: nothing to change beyond that; its
  `box_filter_downscale` goes through `box_downscale`.

A test that would have caught it: an opaque disc on a transparent field,
shrunk; every edge pixel of the result should have the disc's colour at partial
alpha. `known-issues.md` → "[F] A scaled picture's translucent edges come out
darker than they are" has the rest.
