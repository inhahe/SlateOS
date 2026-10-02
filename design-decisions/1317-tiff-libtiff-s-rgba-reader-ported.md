## 1317. TIFF: libtiff's RGBA reader, ported

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous).

**In short:** TIFF pictures -- scans, print and photo exports, screenshots
from scientific tools -- now open. No browser shows a TIFF, so the picture a
TIFF "should" look like is the one the image viewers on free desktops show,
and they all get it from libtiff. `imagecodec` now reads a TIFF the way
libtiff does, line for line, and agrees with a real libtiff on every test
file to the last bit -- including which damaged files it refuses. Some kinds
of TIFF are not read yet (listed below) and are refused by name.

### What it is

`gui/imagecodec/src/tiff/`: a port of libtiff 4.7.1 --

- `dir.rs`: the header (classic, BigTIFF, and Microsoft's "EP" variant) and
  `TIFFReadDirectory`: which tags must read cleanly for the file to open,
  which are dropped with a warning, how every entry type converts, and the
  repairs libtiff makes (missing or implausible `StripByteCounts` estimated,
  surplus colour channels made extra samples, a palette image without a
  palette made grey or RGB).
- `read.rs`: `TIFFFillStrip`/`TIFFFillTile` and the codecs -- none,
  PackBits, Deflate, LZW in `lzw.rs` (both the TIFF 6.0 codes and the
  old-style ones libtiff still reads), and CCITT fax in `fax.rs` (Group 3 1-D
  and 2-D, Group 4, Modified Huffman byte- and word-aligned: `tif_fax3.c`'s
  macros written out, its code tables built as `mkg3states` builds them), and
  JPEG through this crate's port of libjpeg-turbo (§1318) driven as
  `tif_jpeg.c` drives libjpeg, and NeXT's 2-bit and ThunderScan's 4-bit
  codecs in `next.rs` and `thunder.rs` -- with the horizontal predictor,
  `FillOrder`, and big-endian 16-bit samples.
- `rgba.rs`: `tif_getimage.c` -- `TIFFRGBAImageOK`, `TIFFRGBAImageBegin`, the
  strip and tile readers and their pixel routines: grey of 1 to 16 bits,
  palettes, RGB of 8 and 16 with each kind of alpha, CMYK, `YCbCr` (all seven
  subsamplings libtiff converts), CIE L*a*b*, planes together or apart.
- `color.rs`: `tif_color.c`'s `YCbCr` and L*a*b* conversions, in libtiff's
  own single-precision order so they agree to the bit; its one call to
  `pow` is a table generated with the C library libtiff runs on.

### The choices with two sides

1. **libtiff's RGBA interface as the reference.** It is what gdk-pixbuf
   (GNOME's viewer, and most GTK programs) calls. *Against:* macOS's ImageIO
   and Windows' WIC decode differently in places, but they are closed; Qt's
   handler uses libtiff too, through other paths for some layouts. libtiff is
   open, the most used, and can be run: every fixture's answer is a real
   libtiff's, built with the codecs a distribution builds it with.
2. **Its conversions kept, crude ones included.** CMYK becomes RGB by
   `(255-K)(255-C)/255` with no colour profile; 16-bit grey keeps its high
   byte while 16-bit RGB rounds; `FillOrder` 2 reverses the bits of 8-bit data
   too; an uncompressed first tile whose bit-reversed read buffer (rounded up
   to 1024 bytes) is not its size is refused. Each is what the user of a
   libtiff viewer sees, and "better" guesses would be a third behaviour
   agreeing with nobody.
3. **Refused on the first strip that will not read.** gdk-pixbuf asks libtiff
   to stop at the first error and then shows nothing; the alternative, a
   partial picture, would need rules libtiff does not have (its
   carry-on mode leaves stale rows).
4. **Turned truly by `Orientation`.** libtiff's reader only flips (5-8 are
   read as 1-4); gdk-pixbuf then applies the rest. The picture that reaches
   the screen is the tag's, all eight values, which is what `decode` returns;
   `decode_libtiff_raster` gives libtiff's flipped raster for the tests.
5. **Straight alpha, as the file holds it.** libtiff premultiplies
   unassociated alpha into its raster and passes associated alpha through;
   the compositor wants straight alpha, so unassociated alpha is kept exactly
   and associated alpha divided back out (to the value that premultiplies
   back to libtiff's, exhaustively checked). Grey with alpha libtiff passes
   through unpremultiplied either way; that is kept.
6. **Deflate through the shared `deflate` crate, not a second inflater.**
   libtiff decompresses with libdeflate, which stops at the first piece of
   the stream that will not fit the strip; `deflate` decodes a block at a
   time. The two agree on every well-formed file; on some damaged ones they
   do not (`known-issues.md`), and the exact behaviour is asked of lane A
   (`requests/f-a-deflate-decode-into-a-fixed-buffer-as-libdeflate-does.md`)
   rather than written a second time here. *2026-09-26:* answered (lane A's
   4e0b7f205, §967) and in use: a request for the whole of a strip or tile
   -- the strip's own rows, so the short last strip too -- goes through the
   crate's libdeflate port, a request for less through its zlib port, as
   `ZIPDecode` chooses; damaged strips now decode as libtiff decodes them.

### Not yet read

Nothing libtiff's reader decodes is refused now; that took the rest of the
day. (`YCbCr` and CIE L*a*b* samples followed on the same day, held the same way:
23 more fixtures, and 12,000 mutants of them without a disagreement. So did
fax, whose leniency is kept whole -- a bad code word ends only its row, a
Group 4 strip cut short keeps the rows it has, and a Group 3 strip whose data
runs out is decoded again from its start without end-of-line codes, into the
rows still to fill, which can show a cut file whole and wrong; and a fax
tile that fails is shown as far as it decoded, because libtiff tests a
tile's decode for truth where it tests a strip's for success, and fax fails
with -1: 25 fixtures, from libtiff's own encoder, and 12,000 mutants. And so
did JPEG, once JPEG itself was libjpeg-turbo's (§1318): each strip's
datastream checked against its strip as `JPEGPreDecode` checks it, a last
strip's full-height datastream tolerated, the subsampling read from the first
strip's frame when the tag is missing, `JPEGTables` parsed once, and the
tables kept from strip to strip as libjpeg keeps them, so a strip that
redefines them -- even after its scan, which `jpeg_finish_decompress` reads --
redefines them for the abbreviated strips after it. libtiff ignores what that
finish says: it returns `rows_left || finish()`, and a C `||` is 1 for the
failure's -1. 38 fixtures, 30 of them decoded, and 12,000 mutants; and five
more with lossless strips, which libtiff reads as it reads any other -- except
in a `YCbCr` file, where libjpeg will not convert them. NeXT and ThunderScan
followed, with libtiff's edges: a NeXT strip whose data stops at a row's
start is white from there and reads, a NeXT tile's rows are measured by the
image's scanline, not the tile's, and ThunderScan has no tile decoder at all.
libtiff writes neither, so their 27 fixtures come from encoders in the
generator; 12,000 mutants. Then old-style JPEG -- TIFF 6.0's first JPEG
scheme, superseded in 1995 and written every which way: libtiff reads it by
building one JPEG out of the file, its tables and frame from
`JPEGInterchangeFormat` or made up from the tags, then every strip's data
with restart markers put back between strips, and this builds the same one
for the libjpeg-turbo port, which gained raw output and libtiff's strict
source for it (§1318). libtiff's quirks come along: the subsampling is read
from the JPEG's own frame when the directory is read; a big-endian file
loses the codec's post-decode step to the byte swap, so each strip after the
first skips a strip's worth of the JPEG; tiles past libtiff's one-column
frame show what its buffer last held. 26 fixtures -- three table layouts,
tiles, planes apart, and those quirks -- and 20,000 mutants, which found two
things libtiff does that this did not: a strip array the directory lacks
fails the read, and an alpha plane promised past a separate-planes file's
samples is read from strip 0 (`TIFFComputeStrip`), whatever the codec.
Three corners are not modelled, each needing input built to reach it
(`known-issues.md`). Then SGI LogLuv, whose codec turns high-dynamic-range
luminance and chroma into the 8-bit grey or RGB the reader asks for, through
`exp` -- exact only as glibc's `exp` is, which here is a correctly rounded
double-double `exp` plus glibc's own answers for the 21 of its 33,790
possible arguments where glibc is not correctly rounded, checked against
glibc for every one; and through `sqrt`, done in integers, the crate having
no maths library. 16 fixtures, written by an encoder in the generator, and
8,000 mutants. Last, PixarLog: 11-bit log codes, differenced and deflated,
turned to 8- or 16-bit samples through tables libtiff builds with glibc
(kept here as glibc built them), with its bugs shown as it shows them -- for
grey and grey with alpha, each row's last sum spills into the next row's
first pixel, and a tile's rows are the image's width -- and the predictor
it installs run over the output. It inflates with zlib, whose stopping
place the shared `deflate` crate could not find, so on damaged strips the two
could differ (`known-issues.md`; asked of lane A, and since 2026-09-26 done
with the crate's port of zlib's inflate). 18 fixtures, 12,000
mutants.) Only the first page of a multi-page TIFF is read -- as gdk-pixbuf
reads it.

### How it is held

`tests/tiff.rs` against 256 fixtures (`tests/data/generate_tiff.py`: a
small TIFF writer for every layout, plus Pillow's libtiff-backed writer for
real encoder output), each answered by libtiff 4.7.1 built from pinned
sources: 209 decoded to exactly libtiff's raster, 47 refused where libtiff
refuses. Separate tests hold the straight-alpha conversion to libtiff's
premultiplied raster, the eight orientations to the stored picture turned,
limits, and every bit flip of eight fixtures to not panicking. A mutation
fuzzer against the same libtiff -- bit flips, entry types, counts and values
changed, entries dropped and duplicated, files cut short -- found no
disagreement in 30,000 files without Deflate data, and in 8,000 with it only
the nine Deflate cases above; the rounds for `YCbCr` with CIELab, for fax, for
JPEG and for NeXT with ThunderScan (12,000 each), and for lossless JPEG strips
(6,000), found none once the two port errors the fax round turned up were
fixed -- `RowsPerStrip` also sets the tile size while no tile tags have
been read, and the tile truth test above.
