### [F] A damaged PixarLog TIFF strip can be refused where libtiff shows it -- 2026-09-25 -- **FIXED 2026-09-26**

**Status:** FIXED 2026-09-26 — `pixarlog::inflate` is a call to lane A's
`deflate::zlib_inflate_into` (4e0b7f205, the answer to the request below),
which stops where zlib 1.3 stops; `check_trailer` is gone. Checked with the
Deflate entry above: 0 disagreements with libtiff in 9,000 mutants.

**In short (as it was):** libtiff inflates a PixarLog strip with zlib, which stops as
soon as the strip's buffer is full; the shared `deflate` crate decodes a
whole Deflate block at a time, so damage after the part of a block the strip
needs -- or a strip cut just short of its end-of-block code, which zlib
never needs -- makes this refuse a picture libtiff shows. Undamaged files
are unaffected.

**Where.** `gui/imagecodec/src/tiff/pixarlog.rs`, `inflate` and
`check_trailer`.

**How to see it.** The lane F TIFF fuzzer over `tests/data/tiff_pixarlog_*`
(`ONLY=pixarlog`), answered by libtiff 4.7.1: about 1 in 1,000 mutants,
`LIBTIFF DECODES, WE REFUSE: Corrupt("TIFF PixarLog data")`, every one of
them a strip zlib fills before it reaches the damage.

**The proper fix.** A zlib-exact "inflate into this buffer" in `deflate`
(the request above), then `pixarlog::inflate` becomes a call to it and
`check_trailer`'s search for the end of the Deflate data goes.
