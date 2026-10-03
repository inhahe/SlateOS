### [F] A PNG decodes at about twice Pillow's cost, and inflating it is nearly all of that -- 2026-09-26

**Status:** OPEN — the part left is not lane F's code: waiting on lanes A and B
(`requests/f-ab-inflate-decodes-a-bit-at-a-time-five-times-slower-than-zlib-ng.md`).

**In short:** opening a big PNG costs about twice what Pillow (libpng-class C)
spends on it. A 2000x1500 photograph: 0.88 billion cycles of the decoding
thread against Pillow's 0.41. Until 2026-09-26 it was 1.1 to 1.26; this
lane's own half — unfiltering the rows and turning them into pixels — now
costs about 0.1 where Pillow's costs 0.26, so what is left is the inflate: the
shared `deflate` crate's takes 0.78 where zlib-ng takes 0.15. The pixels are
right; only the speed is behind.

**Where.** `deflate/src/lib.rs`, `HuffmanTable::decode`, which reads one bit
at a time in the manner of zlib's `puff.c` (a reference decoder, written to be
read). A root leaf crate no lane owns (`open-questions.md` A-Q11), with lane
B's streaming rewrite of the same code in flight (`454a9bfb5` on `lane-b`),
hence the request rather than an edit.

**Done here** (2026-09-26, pixels held to the snapshot before it on 6,000
random PNGs of every layout, full size and scaled, and to the Pillow
fixtures):
1. Rows unfiltered by one instance of each filter per pixel size (the six
   PNG has), the pixel an array the compiler keeps in registers; Paeth in
   libpng's branch-free form, held to the RFC's on all 2^24 inputs by a test.
2. Rows converted to pixels a row at a time by a converter chosen once from
   the header (palette and transparency folded in beforehand), not a
   colour-type dispatch and two bounds checks per sample per pixel.
3. The orientation lookup no longer checksums the first `IDAT` it stops at —
   every decode asks for the orientation, and a file written as one `IDAT`
   was checksummed twice.
4. The chunk CRC is the `crc32` crate's, not a private copy of its table.

**The proper fix, the rest:** table-driven Huffman decoding in `deflate`, as
the request lays out; then the CRC (byte-at-a-time: 0.033 of the 0.1 left
here), which is the `crc32` crate's to speed up.

**How to see it.** `target/perfbench` against a snapshot (thread cycles, the
minimum of N), Pillow's `Image.open(...).load()` measured the same way, and
`target/inflatebench` for the inflate alone.
