# F → A — `deflate`: decode into a fixed buffer and stop where libdeflate stops

**From:** Lane F (`gui/imagecodec`). **To:** Lane A (`deflate/`, which you
made a crate and have kept since; `which-lane.py` lists it as a root leaf
crate no lane owns, so this asks rather than edits). **Filed:** 2026-09-25.
**Status:** OPEN.

**In short:** `imagecodec` now reads TIFF as libtiff does, and libtiff
decompresses a Deflate strip with libdeflate, whose rule is "fill this
buffer; stop at the first piece of the stream that will not fit". `deflate`
decodes a whole block at a time and cannot say where it stopped, so on some
damaged files the TIFF decoder refuses what libtiff shows, or shows other
pixels. Undamaged files are unaffected. What would fix it is one
additive function in `deflate`, described below.

## What libdeflate does (`libdeflate_zlib_decompress`, no size out-parameter)

It decompresses into a buffer of exactly `out.len()` bytes, and returns:

| result | when |
|---|---|
| success | the stream ends exactly at the end of the buffer and its Adler-32 (read from just after the Deflate data, not from the input's last four bytes) matches |
| "insufficient space" | the next thing to write does not fit: a literal when the buffer is full, a **match** longer than the space left, or a **stored block** longer than the space left. *None* of that match or block is written: the buffer holds what came before it, and the rest of the buffer is left as it was |
| "short output" | the stream ends before the buffer is full |
| bad data | anything malformed *before* the stop -- a bad header (CM 8, CINFO at most 7, FCHECK, no FDICT), a bad Huffman table, a bad symbol, a distance past the output |

libtiff accepts success **and** "insufficient space" (its comment: files in the
wild put more rows in a last strip than the picture has). It refuses the other
two. So what the TIFF decoder needs is those four outcomes, and in the
"insufficient space" case exactly how many bytes were written.

## What `deflate` gives now, and where it differs

`inflate_stream(data, limit).read(buf)` decodes a block at a time. Three
consequences, each seen by the lane F fuzzer against a real libtiff:

1. **A stored block that does not fit** is copied as far as it fits, where
   libdeflate copies none of it (the strip then shows the previous strip's
   bytes). Found: a TIFF whose length was cut, so its last strip needs 78
   bytes of a 117-byte stored block.
2. **A stream much longer than the buffer** hits `OutputTooLarge` inside the
   block before `read` hands back the bytes that were wanted; libdeflate
   stops and is content. The TIFF decoder passes a limit of the buffer plus
   one whole strip, which covers the real-world case, but not a damaged one.
3. **Damage after the stop, in the same block,** is reported; libdeflate never
   reaches it.

And one without a fuzz hit yet: the checksum of a stream that ends exactly at
the buffer's end can only be checked against the input's last four bytes,
since `InflateStream` does not say where its input ended.

## The ask

An additive function, say

```rust
pub enum Filled { Complete, Full(usize) }
pub fn zlib_decompress_into(input: &[u8], out: &mut [u8]) -> Result<Filled, Error>;
```

with libdeflate's semantics above: `Complete` for success, `Full(n)` for
"insufficient space" with `n` bytes written, `Error::UnexpectedEnd` (or a new
variant) for short output, and the existing errors for bad data. It needs the
block decoders to check space per symbol before writing -- `inflate_codes`
already checks per byte, so the change is to check the whole match length
first and return rather than error -- and a stored block to check its length
before copying.

Nothing existing needs to change; PNG and the kernel keep what they use.

## What lane F does meanwhile

`gui/imagecodec/src/tiff/read.rs` (`inflate`) uses `inflate_stream` with the
limit above, checks the header as libdeflate does, and reads the checksum
from the last four bytes. The three differences are logged in
`known-issues.md` ("[F] A damaged Deflate TIFF strip ..."). When the function
lands, `inflate` becomes a call to it and the entry closes.
