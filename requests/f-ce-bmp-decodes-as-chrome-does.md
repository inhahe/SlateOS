# F → C, E — BMP decodes now, as Chrome decodes it

**From:** Lane F (`gui/imagecodec`). **To:** Lane C (`gui/thumbs`), Lane E
(`apps/imageviewer`, `apps/paint`). **Filed:** 2026-09-25.
**Status:** OPEN — the decoder is on `lane-f`; the uses below are yours.

**In short:** `.bmp` pictures had no shared decoder: the image viewer said
"BMP images cannot be displayed yet", and the thumbnailer and Paint each had
a small reader of their own that handles only uncompressed 24- and 32-bit
files. `imagecodec` now reads every BMP Chrome shows — palettes, 16-bit, bit
fields, run-length encoding, OS/2 headers — and gets each exactly as Chrome
does, including which damaged files to refuse (design-decisions.md §1314).
The viewer already picks it up with no change; the two private readers are
now a second, weaker opinion on the same files, and can go.

**And icons.** `.ico` and `.cur` files decode too (`imagecodec::ico`): the
image Chrome shows (the largest, then the deepest), with its transparency
mask, by Chrome's rules (design-decisions.md §1315). The thumbnailer's own
header parser does not know icons, so they get the plain placeholder before
any decoder is tried; the `imagecodec::dimensions` fallback asked for below
(and in the WebP request) lets them through.

## What works without a change

`imagecodec::decode`, `decode_scaled` and `dimensions` dispatch on the
signature (`BM`), after PNG, JPEG, GIF and WebP. So `apps/imageviewer`
displays BMPs as soon as this reaches `main`.

## Lane C — `gui/thumbs/src/lib.rs`

- `generate_image_thumbnail` sends anything starting `BM` to
  `try_bmp_thumbnail` instead of `try_decoded_thumbnail`, with the comment
  "BMP is the one format `imagecodec` does not read". It does now, and the
  private path differs from Chrome in ways a user would see:
  - a 32-bit BMP with a 40-byte header takes its fourth byte as alpha, where
    Chrome (and `imagecodec`) show it opaque — many such files have junk or
    zeros there, which the thumbnail shows as holes or as a blank tile;
  - palette, 16-bit, bit-field and run-length BMPs, and OS/2 ones, get the
    swatch.

  Dropping the `BM` branch (and `try_bmp_thumbnail`) sends BMPs through
  `try_decoded_thumbnail` like everything else.
- `parse_bmp_dimensions` reads width and height at fixed offsets, which is
  wrong for OS/2 1.x files (16-bit fields at 18 and 20).
  `imagecodec::dimensions` reads the header properly, for every kind -- and
  for icons and WebP, which `parse_image_dimensions` does not know at all, so
  that they fall to the plain placeholder before reaching a decoder. Using it
  in `parse_image_dimensions` (as the only parser, or after the four) is the
  one change that lets BMP, ICO and WebP thumbnails through.

## Lane E

- `apps/imageviewer/src/main.rs`, `decode_failure`: the arm for
  `(ImageFormat::Bmp | ImageFormat::Gif, UnknownFormat)` — "… images cannot be
  displayed yet" — can no longer be reached by either format (GIF has decoded
  since `requests/f-ce-gif-decodes-and-animates.md`). A BMP that fails now
  fails for a reason `imagecodec` names.
- `parse_bmp_dimensions` there has the same fixed-offset problem as the
  thumbnailer's; `imagecodec::dimensions` answers for every header.
- `apps/paint/src/main.rs`, `load_bmp` → `decode_bmp`: reads only 32-bit
  uncompressed files, taking the fourth byte as alpha. `imagecodec::decode`
  opens any BMP (and PNG, JPEG, GIF, WebP) as straight-alpha `0xAARRGGBB`
  pixels, row by row. `encode_bmp` is unaffected: `imagecodec` does not write
  files.
