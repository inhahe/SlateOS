## 967. `deflate` inflates into a fixed buffer twice: once stopping exactly where zlib stops, once exactly where libdeflate stops -- overrun bytes included

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous; the two function shapes are lane F's proposals, requests/f-a-zlib-inflate-into-a-fixed-buffer-as-zlib-does.md and f-a-deflate-decode-into-a-fixed-buffer-as-libdeflate-does.md) &middot; **Lane:** A

**In short:** a TIFF picture is stored in compressed strips, and libtiff
decompresses each strip into a buffer of a known size with one of two
libraries -- zlib for one compression type, libdeflate for another. On an
undamaged file both simply fill the buffer. On a damaged one they stop in
different places and leave different bytes behind, and what a user sees --
the picture libtiff shows, or its refusal -- depends on exactly where. The
`deflate` crate now has one function that behaves exactly as each library
does, down to the bytes left in the buffer when decoding stops early, so the
image decoder can show what libtiff shows.

**What was decided.**

1. **Two functions, not one with a mode.** `zlib_inflate_into` is zlib 1.3's
   `inflate()` as libtiff 4.7.1's `PixarLogDecode` drives it;
   `zlib_decompress_into` is libdeflate 1.24's `libdeflate_zlib_decompress`
   with no size out-parameter, as `tif_zip.c` calls it. The libraries
   disagree on seven points of validity alone (litlen 286/287, distance
   30/31, oversize headers, empty and single-codeword codes, a missing
   end-of-block, when a repeat overrun is caught -- the table in
   `src/fixed_buffer.rs`), so a single decoder could match at most one.
2. **Ports of decisions, not of code.** Each decodes canonically, bit by
   bit, but follows its library's order of checks, its acceptance rules for
   codes, and its rules for running out of input or room. For libdeflate
   that includes reading past the end of its input as zero bits and refusing
   at its refill points: its 64-bit buffer holds `56 + (-p mod 8)` bits after
   any refill at bit position `p`, whatever came before, so the emulation
   keeps only the refill *points*, copied from `decompress_template.h`.
3. **libdeflate's match-copy overrun is reproduced.** Its fastloop copies a
   match whole words at a time and runs up to 39 bytes past it. When decoding
   then stops for lack of room, those bytes stay in the buffer, and libtiff
   shows the buffer. So the fastloop's entry and exit conditions, its refill
   schedule and its three copy variants are emulated exactly.
4. **Held to the libraries, not to the specification.** `tests/fixed_buffer.rs`
   replays 1,519 streams against answers recorded from zlib 1.3 and
   libdeflate 1.24 built from pinned sources (`tests/data/fixed_buffer/
   generate.py`). Before the corpus was cut, 105,000 generated streams --
   valid, mutated, truncated, and built with long codewords to reach
   libdeflate's subtables -- were run against both libraries, whole output
   buffers compared, with no disagreement; path counters confirmed the
   fastloop, all three overrun copies and every stopping rule were reached.

**Two points where the requests' descriptions and the libraries differ.**
The implementation follows the libraries:

* zlib does **not** check a match's "distance too far back" once the buffer
  is full: the check sits in its MATCH state after the room check, and
  `inflate_fast` never runs with fewer than 258 bytes of room.
* libdeflate's "insufficient space" does **not** leave the rest of the
  buffer untouched: the fastloop's overrun can have written up to 39 bytes
  past the stopping point.

**Alternatives considered.**

| Option | For | Against |
|---|---|---|
| Extend `inflate_stream` with a fixed-buffer mode | One decoder to maintain | It decodes a block at a time; neither library does, and the seven validity differences cannot all be one decoder's |
| Implement the behaviour the requests describe | Short, readable | Both descriptions were wrong in one detail each, and a TIFF strip that hits either detail would decode differently from libtiff |
| Link zlib and libdeflate | Exact by construction | C in the crate the kernel links, a build that needs a C toolchain for every target, and no answer at all for `no_std` |
| **Exact ports, differentially tested** (chosen) | Exact on everything tested, and the tests say where | ~1,200 lines that must track two libraries' versions; the pinned sources in `generate.py` say which |

**Where this bites:** `deflate/src/fixed_buffer.rs`, `deflate/src/lib.rs`
(`Error::ShortOutput`), `deflate/tests/fixed_buffer.rs`,
`deflate/tests/data/fixed_buffer/`.
