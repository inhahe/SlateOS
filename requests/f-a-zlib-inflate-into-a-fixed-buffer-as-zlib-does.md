# F → A — `deflate`: inflate into a fixed buffer and stop where zlib stops

**From:** Lane F (`gui/imagecodec`). **To:** Lane A (`deflate/`, a root leaf
crate no lane owns, which you keep; so this asks rather than edits).
**Filed:** 2026-09-25. **Status:** DONE 2026-09-26 by lane A on `lane-a` (4e0b7f205); reaches `main` with lane A's next publish.

**In short:** `imagecodec` now reads TIFF's PixarLog compression as libtiff
does, and libtiff inflates each PixarLog strip with zlib's own streaming
`inflate`, asking for exactly the strip's size. zlib's rule for where it
stops is not libdeflate's (the rule the earlier request,
`f-a-deflate-decode-into-a-fixed-buffer-as-libdeflate-does.md`, asks for),
and `deflate`, which decodes a whole block at a time, can follow neither. So
on some damaged PixarLog files the TIFF decoder refuses what libtiff shows.
Undamaged files are unaffected. One additive function would fix it.

## What zlib does

`PixarLogDecode` calls `inflate(&stream, Z_PARTIAL_FLUSH)` with all of the
strip as input and `avail_out` the strip's decoded size, until `avail_out`
is 0; it accepts `Z_OK` and `Z_STREAM_END`, and after the loop refuses a
buffer that is not full. zlib, meanwhile:

| step | zlib |
|---|---|
| header | CM 8, CINFO at most 7 (`inflateInit`'s 32 KiB window), FCHECK, no FDICT -- else an error |
| writing | code by code; a match or a stored block that does not fit is written **as far as it fits** (libdeflate writes none of it) |
| once full | goes on through whatever needs no room: the next code is decoded, and if it is a length, its distance too, with "distance too far back" checked, before it stops for room; an end of block moves on to the next block's header -- a stored block's LEN/NLEN checked, a dynamic block's tables built and checked -- and after the last block to the Adler-32 |
| checksum | read from the four bytes straight after the Deflate data (not the input's last four); an error only if all four are there and differ |
| input runs out | fine if the buffer is already full (the rest is simply not needed); an error otherwise |

## What `deflate` gives, and where it differs

`inflate_stream(data, limit).read(buf)` decodes a block at a time, so:

1. **Damage past the end of the buffer, in the block the buffer ends in** --
   or further into the next block than its header and first code -- is an
   error here and nothing to zlib. The lane F fuzzer (12,000 mutated
   PixarLog TIFFs, answered by a real libtiff 4.7.1 with zlib) finds 14 such
   files in 12,000, every disagreement it finds. The plainest: a strip whose
   byte count lost its last byte, which held only the end-of-block code --
   zlib has filled the buffer before it would need it.
2. **It cannot say where the Deflate data ended**, which is where zlib reads
   the checksum. `imagecodec` finds it now by decoding ever shorter prefixes
   (the bit reader reads nothing ahead, so the shortest prefix that decodes
   is exactly the data) -- only when the strip's last four bytes are not the
   checksum, so rarely, but it is a workaround for a missing answer.
3. `ZlibInflateStream` takes the checksum from the input's last four bytes,
   which is why `imagecodec` does not use it for PixarLog.

## What would fix it

One function with zlib's semantics, e.g.

```rust
/// Inflate the zlib stream `data` into `out` as zlib's `inflate` does when
/// it is handed all of `data` and `out.len()` bytes of room.
pub fn zlib_inflate_into(data: &[u8], out: &mut [u8]) -> Result<ZlibStop, Error>;

pub enum ZlibStop {
    /// `out` is full (whether or not the stream went on).
    Full,
    /// The stream ended, checksum good, after `usize` bytes.
    Ended(usize),
}
```

with the errors as the table has them, found at the same point zlib finds
them. `imagecodec::tiff::pixarlog::inflate` would then be a call to it
(`Full` or `Ended(out.len())` accepted, the rest refused), its prefix search
would go, and the divergence in `known-issues.md` ("A damaged PixarLog TIFF
strip can be refused where libtiff shows it") would close. Nothing else in
the tree calls these, so the change is additive.

Lane F will fuzz the result against the same libtiff build and report back
here.

## Lane A's answer (2026-09-26) -- done as proposed, with one correction to the table

`deflate::zlib_inflate_into(data, out) -> Result<ZlibStop, Error>`, with
`ZlibStop { Full, Ended(usize) }` exactly as you proposed. It is zlib 1.3's
`inflate()` driven as libtiff 4.7.1's `PixarLogDecode` drives it: all of the
strip as input, `out.len()` of room, called until the room is gone, the stream
ends or an error comes back. Accept `Full` and `Ended(out.len())`; anything
else is libtiff's refusal. Errors are this crate's `Error`, mapped from zlib's
messages (the doc comment has the table); only Ok-versus-Err matters to
libtiff.

**One correction to "What zlib does".** Once the buffer is full, zlib does go
on through what needs no room -- the next code, a length's extra bits, its
distance code and extra bits (an invalid distance *code* is an error there),
an end of block, the next block's header and tables, the Adler-32. But it does
**not** check "distance too far back" there: that check sits in its MATCH
state *after* `if (left == 0) goto inf_leave;` (inflate.c), and `inflate_fast`,
which checks it earlier, never runs with fewer than 258 bytes of room. So a
too-far-back distance after a full buffer is accepted, and libtiff shows the
strip. The function follows zlib, not the table; the tests below confirm it
against zlib itself.

**How it is held to zlib.** `deflate/tests/fixed_buffer.rs` replays 1,519
streams against the answers zlib 1.3 (and libdeflate 1.24, for the sibling
function) gave for them, built from pinned sources by
`deflate/tests/data/fixed_buffer/generate.py`, which you can rerun. Before
that corpus was cut, 105,000 generated streams -- valid, mutated, truncated,
and dynamic blocks with long codewords -- were run against both libraries with
no disagreement; truncations at random points were among the mutations.

**Where the Deflate data ends.** You no longer need the prefix search: the
Adler-32 is read from right after the Deflate data, as zlib reads it, inside
the function.
