### [F] A damaged Deflate TIFF strip can decode otherwise than in libtiff -- 2026-09-25 -- **FIXED 2026-09-26**

**Status:** FIXED 2026-09-26 — `tiff/read.rs`'s `inflate` is now two calls
into lane A's fixed-buffer inflates (4e0b7f205, the answer to the request
below): `deflate::zlib_decompress_into`, libdeflate 1.24's decisions, when
the reader asks for the whole of a strip or tile, and
`deflate::zlib_inflate_into`, zlib 1.3's, when it asks for less -- the
choice `ZIPDecode` makes. Switching over, the fuzzer found one more thing
this entry had wrong: libtiff's "whole strip" is the strip's *own* rows, so
the shorter last strip goes to libdeflate too, not to zlib (an
over-full last strip then shows the strip before it, where libdeflate
writes nothing). Fixtures `tiff_grey8_deflate_last_strip_holds_more_stored`
and `..._match` pin that. Checked: 9,000 mutants of the Deflate, ZIP and
PixarLog fixtures against libtiff 4.7.1, whole pixels compared -- 0
disagreements.

**In short (as it was):** a TIFF whose Deflate-compressed data is longer than its strip,
or damaged after the part the strip needs, can be refused here where libtiff
shows it, or shown with different pixels in its last strip. Undamaged files
decode identically; it takes a damaged or hand-made file.

**Where.** `gui/imagecodec/src/tiff/read.rs`, `inflate`. libtiff decompresses a
strip with libdeflate, which fills the strip and stops at the first piece of
the stream that will not fit -- writing none of a match or stored block that
does not, and never looking at what follows. The shared `deflate` crate
decodes a block at a time, so it copies the part of a stored block that fits
(where libdeflate leaves the strip's buffer as it was), sees damage later in
the block, stops with `OutputTooLarge` when a block holds far more than the
strip, and cannot say where its input ended, so the checksum of a stream that
ends exactly with the strip is read from the input's last four bytes.

**How to see it.** The lane F TIFF fuzzer (mutated copies of
`tests/data/tiff_*deflate*` and `*zip*`, answered by libtiff 4.7.1): about
one mutant in a thousand, every one of them a strip cut shorter than its
stream or a stream damaged past the strip's end.

**The proper fix.** An additive `deflate` function with libdeflate's
semantics (the request above spells them out); `inflate` becomes a call to it.
Not a second inflater in `imagecodec`: design-decisions §555.
