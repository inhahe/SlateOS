### [F] A lossless alpha plane cut off mid-symbol can differ from libwebp in its last pixel -- 2026-09-25 -- **FIXED 2026-09-25**

**Status:** FIXED — `Bits` in `webp/lossless.rs` is now a port of libwebp's `VP8LBitReader`, called where `vp8l_dec.c` calls it, and the byte-per-pixel alpha loop is libwebp's `DecodeAlphaData`; `tests/data/webp_lossy_alpha_corrupt_tail.webp` is the file that showed it. The original report follows.

**In short:** on a damaged file only. If a WebP's alpha plane is stored
compressed, is one libwebp decodes a byte per pixel, and runs out of data in
the middle of its very last pixel, libwebp still shows the picture -- and so
does this -- but libwebp fills that one pixel from bits it has already used,
and this fills it from zeros. One pixel's transparency in one corner of a
damaged file. Found by comparing 5,200 corrupted files with libwebp
(design-decisions.md §1312); it was the only difference left.

**Where.** `gui/imagecodec/src/webp/lossless.rs`, `Bits`: past the end of
the data it yields zeros; libwebp's `VP8LBitReader` wraps its 64-bit window
(`bit_pos & 63`) once a read starts at its 64th bit, and after `eos` is set it
resets `bit_pos` to 0 and reads the window from its start.

**The proper fix.** Make `Bits` a port of libwebp's `VP8LBitReader` -- its
64-bit window, `ShiftBytes` and `DoFillBitWindow`, `PrefetchBits` masking the
position, `SetBitPos` without checks, `ReadBits` returning 0 once `eos` is
set -- and call it where libwebp's `vp8l_dec.c` does (`FillBitWindow` before
each symbol, the second-level lookup re-prefetching), so the stale bits are
the ones libwebp reads.

**How to see it.** Flip bits in `tests/data/webp_lossy_alpha.webp` with a
seeded generator (the comparison script is described in §1312) and decode
each with Pillow and with `imagecodec::decode`: one file in 5,200 differs, in
its last pixel's alpha.
