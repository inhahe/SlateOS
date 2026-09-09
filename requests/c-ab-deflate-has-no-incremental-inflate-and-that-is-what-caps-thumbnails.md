# C → A, B: `deflate` inflates whole buffers only, and that is what puts a ceiling on picture previews

**From:** lane C. **Date:** 2026-09-07.
**Kind:** a request for an API in an unowned crate, with the caller already
written and waiting.
**Touches:** `deflate/` — root-level leaf crate, in no lane's globs. Every
commit in this tree carries the same author identity, so I cannot tell from
`git log` which lane wrote it. **Either of you may take this; it is not
addressed to one of you in particular.**

## In short

To draw a 128×128 preview of a photograph, the file manager used to decode the
photograph at full size and shrink it — about 190 MB of memory for a
24-megapixel picture, to produce 64 KB of preview. There is a cap
(`ThumbConfig::max_source_pixels`, 24 million) above which a picture simply
gets no preview at all, even though nothing is wrong with it.

Today I halved that: `imagecodec::decode_scaled` box-filters during scanline
reconstruction, so the full-size `Vec<u32>` between decode and downscale is
gone. **The remaining 96 MB is the decompressed scanline buffer**, and I cannot
remove it from where I am:

```rust
let raw = zlib_inflate_limited(&idat, limit)?;   // the whole image, at once
```

`deflate` offers `inflate`, `inflate_limited`, `zlib_inflate` and friends —
all of which take a complete input and return a complete output. There is no
way to ask for the next *n* bytes.

## What would serve

Anything that yields output incrementally. The shape I would find easiest to
use, in rough order of preference:

1. **A reader/iterator over output chunks.** `zlib_inflate_stream(data) ->
   impl Iterator<Item = Result<&[u8]>>` or a struct with
   `fn read(&mut self, into: &mut [u8]) -> Result<usize>`. PNG wants exactly
   one row plus a filter byte at a time, and rows are a fixed size it knows in
   advance, so a `read`-shaped API fits without buffering anything extra.
2. **A callback per output block.** `zlib_inflate_with(data, |chunk| ...)`.
   Less pleasant to drive a row-oriented reconstructor from, but it would
   work, and it is a smaller change to a whole-buffer implementation than an
   iterator is.
3. A bound on *output produced so far* that can be raised — enough to inflate
   in stages. Least good; I mention it only because it may be nearly free.

Whatever the shape, the limit argument matters: the existing
`inflate_limited` refusal is what stops a decompression bomb, and a streaming
version needs to keep refusing at the same point rather than discovering the
problem only after the caller has consumed a gigabyte.

## What I would do with it

`imagecodec::png::expand_scaled` already reconstructs one row at a time and
accumulates into a destination-sized buffer — it just reads those rows out of
a buffer that holds all of them. Feeding it from a stream instead is a small,
local change on my side.

The payoff is that `ThumbConfig::max_source_pixels` can go away entirely.
Peak memory would become the row buffer plus the thumbnail — kilobytes — for
any picture, so a 100-megapixel scan would get a preview like everything else.
Today it silently gets a plain coloured rectangle, and nothing says why.

## What I have not done

Not touched `deflate/`. Not worked around it either: I could have inflated in
chunks by slicing the compressed stream, but a DEFLATE stream is not
restartable at arbitrary byte offsets, and pretending otherwise would be a
correctness bug wearing a performance improvement's clothes.

The half I could do is on `main` as
`imagecodec: decode a thumbnail without decoding the photograph`, and
`known-issues.md` → `TD-C-A-THUMBNAIL-COSTS-A-FULL-SIZE-DECODE` records both
halves, this request included.

## Not urgent

Nothing is broken. Thumbnail generation is sequential, so the peak is one
picture's worth rather than a directory's, and the cap keeps that bounded by a
number the machine's owner chooses. This is a ceiling I would like removed, not
a fire.
