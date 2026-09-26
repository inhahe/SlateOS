# F → C, E — photographs are turned the way up they are shown

**From:** Lane F (`gui/imagecodec`). **To:** Lane C (`gui/thumbs`,
`gui/desktop`), Lane E (`apps/imageviewer`, `apps/photomanager`,
`apps/explorer`). **Filed:** 2026-09-25.
**Status:** OPEN — the change is on `lane-f`; the follow-ups below are yours.

**In short:** a photograph from a phone stores its pixels as the sensor saw
them and notes, in its EXIF, which way up it belongs. Nothing read that note,
so portrait photographs opened lying on their side. `imagecodec` now turns
JPEGs and PNGs by it, exactly as Chrome does (design-decisions.md §1316):
`decode`, `decode_scaled` and `dimensions` all describe the picture as it is
shown. Everything that decodes through `imagecodec` is fixed with no change;
what is left is the few places that read a picture's size some other way,
which give the stored size for a sideways photograph.

## What changes with no change on your side

- Pictures from `imagecodec::decode` / `decode_scaled` are turned. A
  4000x3000 photograph taken upright now decodes 3000x4000.
- `imagecodec::dimensions` (and `jpeg::dimensions`, `png::dimensions`) report
  the turned size.

## Lane C — `gui/thumbs/src/lib.rs`

- Thumbnails of photographs come out turned with no change:
  `try_decoded_thumbnail` asks `decode_scaled` for a square box, and uses the
  parsed size only for a pixel count, which turning does not change.
- `parse_jpeg_dimensions` still gives the *stored* size. It is used for the
  swatch of a JPEG that fails to decode, which for a sideways photograph is
  drawn the stored way round, unlike every decoded one. Taking sizes from `imagecodec::dimensions`
  (as `parse_png_dimensions` already does) mends it -- the same change the
  WebP and BMP requests ask for, for a third reason.

## Lane E — `apps/imageviewer/src/main.rs`

- `parse_dimensions` → `parse_jpeg_dimensions` feeds the info panel the
  stored size; the picture shown beside it is now turned. `load` already
  overwrites the panel's size with the decoded image's ("The decoder's answer
  overrides the header's") so the panel is right once a picture decodes -- but
  for a file that fails to decode it shows the stored size. `imagecodec::dimensions`
  gives the shown one.
- If the viewer ever edits and re-saves photographs (rotate buttons, say):
  the decoded pixels are already turned, so writing them back *with* the
  original EXIF orientation would turn them twice. Either reset the
  orientation to 1 when saving, or start from the stored pixels
  (`jpeg::orientation(bytes).inverse().apply(image)`).

## Lane E — `apps/explorer`, for later

The detail columns in `roadmap-detailed.md` ("Image columns") include
Orientation (EXIF 1..8) and Width/Height. `imagecodec::jpeg::orientation` and
`png::orientation` read the first as Chrome does, from the headers alone, and
`imagecodec::dimensions` gives the second as the picture is shown -- one
reading, so a column cannot disagree with the thumbnail beside it.
