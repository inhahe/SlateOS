# F → C, E — TIFF decodes, as libtiff does

**From:** Lane F (`gui/imagecodec`). **To:** Lane C (`gui/thumbs`), Lane E
(`apps/imageviewer`, `apps/explorer`, `apps/photomanager`).
**Filed:** 2026-09-25. **Status:** OPEN — the decoder is on `lane-f`; the
follow-ups below are yours.

**In short:** `imagecodec::decode`, `decode_scaled` and `dimensions` now read
TIFF (design-decisions.md §1317): grey, palette, RGB, CMYK, `YCbCr` and
CIE L*a*b* pictures of every depth libtiff's viewer path takes, in strips or tiles, uncompressed,
PackBits, LZW, Deflate, CCITT fax (scanned documents) or JPEG, the 1990s
"old-style" kind included (photographs and colour scans), turned by the
`Orientation` tag. The picture is exactly what a libtiff-based viewer
shows, high-dynamic-range SGI LogLuv included. A few rare TIFFs are
refused by name -- `ImageError::Unsupported` -- those of one vendor codec,
PixarLog. Anything that decodes
through `imagecodec` needs no change; the few places that sniff formats
themselves do.

## Lane C — `gui/thumbs/src/lib.rs`

- `parse_image_dimensions` does not know TIFF, so a TIFF gets the plain
  placeholder rather than a thumbnail. Its size cannot come from the first
  1024 bytes anyway: a TIFF's directory -- which holds the size -- is usually
  written *after* the pixels, at the end of the file. `imagecodec::dimensions`
  over the whole file (bounded as `try_decoded_thumbnail` already bounds what
  it reads) gives it, turned the way the picture is shown -- the same change
  the WebP, BMP and orientation requests ask for, for one more reason.
- `try_decoded_thumbnail` then works as for the other formats:
  `decode_scaled` decodes the whole picture and averages it down, as it does
  for BMP and GIF.

## Lane E

- `apps/imageviewer`: `IMAGE_EXTENSIONS` already lists `tif`/`tiff`, and
  `load` decodes through `imagecodec`, so TIFFs open. If `ImageFormat::detect`
  learns the TIFF signature (`II*\0`, `MM\0*`, `II+\0`/`MM\0+` for BigTIFF),
  `decode_failure` can tell "not a TIFF this can show yet" (`Unsupported`)
  from a broken file.
- `apps/explorer/src/columns.rs`: the image columns' `supported_extensions`
  is `png, jpg, jpeg, gif, bmp, svg` -- `webp`, `ico`, `tif` and `tiff` all
  decode now, and `imagecodec::dimensions` answers the Dimensions column for
  every one of them (the column is still a placeholder).
- `apps/photomanager`: it knows `ImageFormat::Tiff` by extension; if it decodes
  its thumbnails through `imagecodec`, TIFFs appear with no change.
