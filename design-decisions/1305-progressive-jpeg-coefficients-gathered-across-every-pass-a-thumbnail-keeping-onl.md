## 1305. Progressive JPEG: coefficients gathered across every pass, a thumbnail keeping only what it draws, and a cut-off file shown as far as it got

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous). Mechanism inside `gui/imagecodec`; the one
call with a user-visible side is how a truncated file is treated, recorded
below.
**Superseded by §1318** the same day: the decoder this describes was replaced
by a port of libjpeg-turbo. Its compact coefficient store for thumbnails lives
on there, and a cut-off file is still shown as far as it got -- now exactly as
libjpeg shows it.

**In short:** photographs saved "progressive" — a large share of those on the
web and many a camera's export — used to be refused outright, so the file
manager showed no thumbnail and the image viewer said it could not open them.
They now decode, to exactly the same pixels as the same picture saved the
ordinary way, and a thumbnail of one costs a fraction of the memory the full
picture would.

### How

`gui/imagecodec/src/jpeg/progressive.rs`. A progressive file sends the picture
as up to a dozen scans, each adding a band of frequencies or one more bit of
precision, so nothing can be turned into pixels until the last scan is in. The
coefficients of every block are gathered in `Coefficients` across all four
kinds of scan (DC first and refinement; AC first, with end-of-band runs, and
refinement, transcribed from T.81 G.1.2.3 and cross-read against libjpeg and
stb_image), then reconstructed through the baseline path's own dequantising,
inverse DCT, upsampling and colour conversion — so the two share every step
after entropy decoding and cannot disagree.

### The choices with two sides

1. **A scaled decode keeps only the coefficients its transform reads, plus a
   one-bit "non-zero" mask for the rest.** *(Since §1307 the transform reads a
   different set -- 7 a side at half scale, 5 at a quarter -- and 4:2:0 chroma
   is reconstructed at twice the picture's block size; the principle is
   unchanged.)* *For:* the whole picture's
   coefficients must be held until the last scan — 136 bytes a block, some 70 MB
   for a 21-megapixel photograph — and a thumbnail's scaled transform reads only
   the top-left `n x n` of each block; an eighth-scale thumbnail needs the DC
   alone, 10 bytes a block. *Against:* two storage shapes. The mask is not
   optional: a refinement scan reads a correction bit for exactly the
   coefficients already non-zero, so parsing needs to know that for every
   position even where the value is thrown away. Held to exactness by a test
   that the scaled progressive decode equals the scaled baseline decode at five
   sizes.
2. **A file cut off early is reconstructed from the passes that arrived**,
   mid-pass included, rather than refused. *For:* that is what progressive is
   for, it is what every browser shows, and the earlier passes are a genuinely
   right picture at lower precision — not the "recognisable and wrong" first
   scan the old refusal worried about, which was about decoding *one* pass of a
   *complete* file. *Against:* a truncated download shows as a soft picture
   rather than an error. Baseline already returns the rows it decoded, so this
   is the same policy.
3. **Quantisation tables are latched per component at its first scan**, as
   libjpeg does: a `DQT` between scans may reuse a table number, and the
   coefficients already sent were quantised with the old table.
4. **The reference comparison covers the unsubsampled fixtures only, for now.**
   Against Pillow, 4:4:4 and greyscale agree to within inverse-DCT rounding
   (worst 2 levels). The subsampled ones differ by up to 100 at sharp colour
   edges — not a progressive defect, since the baseline twin decodes to the
   same pixels, but this crate's nearest-neighbour chroma upsampling against
   libjpeg's interpolating one. That is fixed separately (§1306).
   *Since §1306 the comparison covers every fixture.*

### How it is held

`tests/jpeg_progressive.rs` over six Pillow-written pairs (4:2:0, 4:2:2, 4:4:4,
greyscale with non-interleaved DC passes, a larger picture with partial MCUs,
and restart markers inside every pass): the progressive file decodes to
exactly its baseline twin's pixels, full size and at five thumbnail sizes;
agrees with Pillow where no upsampling intervenes; reads its size from the
header; decodes when cut between passes or mid-pass; survives every truncation
and a spread of bit flips; and is refused past the byte budget at full size
while its eighth-scale thumbnail fits in the same budget.
