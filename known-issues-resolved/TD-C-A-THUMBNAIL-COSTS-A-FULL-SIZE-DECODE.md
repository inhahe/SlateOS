## `TD-C-A-THUMBNAIL-COSTS-A-FULL-SIZE-DECODE` (lane C, 2026-08-26) -- FIXED 2026-09-24 for PNG and JPEG; the cap follows 2026-09-25
**Status:** ✅ FIXED 2026-09-24 (lane F), both halves — `imagecodec` now streams a PNG's rows out of the decompressor (`deflate::zlib_inflate_stream`, which had landed in the meantime), so neither `decode` nor `decode_scaled` holds the decompressed stream, and `decode_scaled` box-filters interlaced files too. 2000x1500 PNG: thumbnail peak 24.6 MB → 0.65 MB, full decode 36.0 MB → 12.1 MB (`gui/imagecodec/tests/decode_memory.rs`). `gui/thumbs`' 24-megapixel cap no longer protects anything: `requests/f-c-a-thumbnail-no-longer-decodes-the-picture-whole-so-the-source-cap-can-go.md`. — **2026-09-25 (lane C): the cap now follows the decoder.** It still protects the formats whose decoder holds the whole picture before shrinking it -- GIF, WebP, BMP, ICO and TIFF, which arrived since and do not stream -- so it stays at 24 megapixels for those (`ThumbConfig::max_source_pixels`, a memory bound). PNG and JPEG are bound instead by `max_streamed_pixels`, 250 megapixels, a bound on time; and every format by `max_source_bytes`, 256 MiB, since the file is still read whole. `streams_while_decoding` in `gui/thumbs` is the list a newly streaming format joins.

**Update 2026-09-07: the peak is halved, and the remaining half is not
reachable from this lane.** `imagecodec::decode_scaled` box-filters during
scanline reconstruction, exactly as this entry proposed, and the thumbnailer
uses it. The full-size `Vec<u32>` between decode and downscale is gone -- for a
24-megapixel photograph, 96 MB of the roughly 190 MB.

**What still costs 96 MB, and why it stays.** The *decompressed scanlines*.
`zlib_inflate_limited` inflates the whole stream in one call before any
reconstruction happens, because `deflate` exposes only `inflate` and
`inflate_limited` over a complete buffer -- there is no incremental API to feed
rows from. Adding one is what would let `ThumbConfig::max_source_pixels` go
away entirely, and `deflate/` is not in lane C's globs. That is the piece to
ask for, not to work around.

**Interlaced files keep the old path**, as the entry predicted: Adam7's passes
arrive scattered across the image, so a row-by-row accumulator has nothing
coherent to accumulate. `decode_scaled` falls back rather than refusing, so a
caller never has to know which case a file is.

**The test that carries the design** asserts a scaled decode gives the same
answer as decoding and *then* averaging, computed independently in the test
rather than read back from the code. Two averaging rules would make the same
photograph look different depending on which path it took. Four more cover the
edges: a picture smaller than the box is not enlarged, the aspect ratio
survives, a 400x3 panorama still yields at least one row rather than rounding
to an empty picture, and every destination cell receives at least one source
pixel -- an off-by-one leaving the last row untouched is almost invisible by
eye on a dark image.

Original entry follows.

---


**In short:** to draw a 128×128 preview of a photograph, the file manager
decodes the photograph at full size first and then shrinks it. A 24-megapixel
picture is about 190 MB of memory for a fraction of a second, to produce 64 KB
of preview. It works, and it is bounded — there is a cap, above which the entry
simply keeps the plain coloured rectangle it always had — but the cap exists
only because the decode is wasteful, and a picture *above* the cap gets no
preview even though nothing about it is wrong.

**Where it lives.** `apps/explorer/src/thumbs.rs` →
`try_decoded_thumbnail`, which calls `imagecodec::decode` and then
`box_filter_downscale`. The cap is `DEFAULT_MAX_SOURCE_PIXELS` (24 million —
a 6000×4000 full-frame photograph), surfaced as `ThumbConfig::max_source_pixels`
so a low-memory machine can lower it.

**Why it costs what it does.** `imagecodec::decode` has no scaled or partial
mode: it holds the compressed `IDAT` copy, the inflated scanline buffer
(`w × h × bytes-per-pixel + h`) and the finished `Vec<u32>` at once, and returns
the last of those at the file's own size. The thumbnailer then converts that
into a `Canvas` and box-filters it down. The conversion is written to consume
the decoder's buffer rather than borrow it (`into_iter().map(...).collect()`
instead of `to_argb_bytes()`), and the compressed data is dropped before it, so
the thumbnailer's own overhead is about as low as it can be *given a full-size
decode*. The full-size decode is the cost.

**Not urgent, and why.** Thumbnail generation is sequential — one file at a
time — so the peak is one picture's worth, not a directory's. The cap keeps
that peak bounded by a number the machine's owner can choose. Nothing crashes
and nothing regresses at the boundary.

**What the proper fix is.** A scaled decode in `imagecodec`: box-filter *during*
scanline reconstruction, accumulating into a destination-sized buffer as rows
come out of the inflater, so peak memory is the row buffer plus the thumbnail
rather than the whole picture. PNG makes this easy in the non-interlaced case
because rows arrive in order; Adam7 needs the full-size buffer anyway, since its
passes are scattered, so that path would keep the current behaviour. With it,
`max_source_pixels` can go away entirely and every picture gets a preview.

**How you would notice.** Open a directory of 100-megapixel scans in the icon
view: they show the plain green rectangle rather than a preview, while the
6000×4000 photographs beside them show properly. Nothing reports why.
