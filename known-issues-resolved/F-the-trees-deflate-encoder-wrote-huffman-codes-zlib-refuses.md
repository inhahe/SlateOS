### [F] The tree's DEFLATE encoder wrote Huffman codes zlib refuses -- 2026-09-27 -- **FIXED 2026-09-27**

**Status:** FIXED 2026-09-27 by lane F in `deflate/src/lib.rs`
(`build_code_lengths`, new `limit_lengths`) -- a crate no lane owns (A-Q11),
fixed there because every user shared the bug; lanes A, B and E notified.

**In short (as found):** data compressed by this tree's `deflate` crate --
gzip and zip files, initramfs images, the kernel's compressed files, PNGs
written by `apps/pngwrite` -- could be unreadable by every other system.
zlib, and so Python, Pillow, browsers and most tools, stopped with "invalid
code lengths set"; this tree's own decompressor read them, so nothing here
noticed. Found writing `imagecodec::encode_png`: Pillow refused a 30x40 RGBA
picture this tree's decoder read back perfectly.

**Cause.** A DEFLATE block with its own Huffman codes stores their lengths,
limited to 15 bits (7 for the code that codes the lengths themselves). Where
the tree's lengths ran past the limit, the encoder shortened the longest code
and lengthened the shortest -- which does not keep the code *complete* (its
lengths' Kraft sum exactly 1). zlib refuses any incomplete code-length code,
and even one lone code of length 1 there (`inftrees.c`). The 19-symbol
code-length alphabet with its 7-bit limit met this often.

**Fix.** Lengths cut to the limit are rebalanced by counting codes per length
-- lengthening the longest short codes until the code is not over-subscribed,
then shortening the longest codes that fit the room left -- and handed out by
frequency, as zlib's `gen_bitlen` does; a lone symbol is paired with another
at length 1. Tests check every code the encoder builds, and every dynamic
block's three codes in its output, as `inftrees.c` would, and fail on the old
code; the callers' suites (ziparchive, zip, logrotate, mkinitramfs,
archivemanager, pdfviewer, explorer, pngwrite: 1,012 tests) pass unchanged.

**Left as it was, on purpose:** the decompressor still accepts an incomplete
code, as it did. zlib would refuse one, but files this encoder wrote before
the fix -- on disk now, some of them the kernel's own compressed files --
must stay readable here. Such a file may still be unreadable elsewhere until
it is written again.
