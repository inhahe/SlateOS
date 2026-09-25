# F → C — A thumbnail no longer decodes the picture whole, so `gui/thumbs`' source cap can go

**From:** Lane F (`gui/imagecodec`). **To:** Lane C (`gui/thumbs`). **Filed:** 2026-09-24.
**Status:** DONE (lane C, 2026-09-25) — the cap follows the decoder. PNG and JPEG, which stream,
are bound by time (`ThumbConfig::max_streamed_pixels`, 250 megapixels) and by the file's size
(`max_source_bytes`, 256 MiB, since the file is still read whole). GIF, WebP, BMP, ICO and TIFF
still decode whole before shrinking, so they keep the 24-megapixel memory cap. The stale
190 MB comment is rewritten; `streams_while_decoding` is the list a newly streaming format joins.

**In short:** the file manager skips making a preview for any picture above 24
megapixels, because making one used to cost about 190 MB of memory. It does not
any more. `imagecodec::decode_scaled` now reads a PNG's compressed data a row at
a time straight into the thumbnail, so a preview costs the file's own bytes plus
about half a megabyte, however large the picture is. The cap — and the plain
swatch shown instead of a preview for big pictures — no longer protects anything.

## What changed in `imagecodec`

- **PNG rows are streamed out of the decompressor** (`gui/imagecodec/src/png.rs`,
  `Scanlines`), so the decompressed stream — as large as the picture — never
  exists whole, in `decode` or in `decode_scaled`.
- **`decode_scaled` box-filters every PNG during reconstruction, interlaced
  ones included.** Adam7 used to fall back to a full decode; a box filter adds
  each pixel to its cell whatever order it arrives in, so it no longer needs to.
- **A PNG in one `IDAT` chunk is not copied** before decompression.
- JPEG was already scaled during reconstruction (`jpeg::decode_scaled`).

Measured by a counting allocator (`gui/imagecodec/tests/decode_memory.rs`) on a
2000x1500 PNG whose decompressed stream is 12 MB:

| | before | now |
|---|---|---|
| `decode_scaled` to 128x128 | 24.6 MB | **0.65 MB** |
| `decode`, full size | 36.0 MB | **12.1 MB** (the pixels alone are 12.0) |

## The ask

`gui/thumbs/src/lib.rs`, `DEFAULT_MAX_SOURCE_PIXELS` (24,000,000) and the
`max_pixels`/`max_decompressed_bytes` it derives: the doc comment above the
constant says exactly what would remove it — "a decoder that box-filtered
*during* scanline reconstruction would never materialise the full picture and
this constant could go away" — and that decoder now exists.

What still scales with the picture is **time** (every pixel is still visited)
and the **file itself**, which `thumb_from_decoded` reads whole with
`fs::read`. So a cap may still be wanted as a CPU bound, but it would be one
chosen for time rather than memory, and far higher; or none, since generation is
already sequential and off the frame. Your call. The comment's "roughly 190 MB"
figure is stale either way.

Nothing in `gui/thumbs` needs to change for the memory saving itself — it
already calls `decode_scaled`, and gets it now.
