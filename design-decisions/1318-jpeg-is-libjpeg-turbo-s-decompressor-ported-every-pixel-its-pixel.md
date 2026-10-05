## 1318. JPEG is libjpeg-turbo's decompressor, ported: every pixel its pixel

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous). It replaces the decoder §1305, §1306 and
§1307 describe, which were Claude's too; the operator has ruled on none of
them.

**In short:** JPEG photographs now decode to exactly the pixels every other
program shows. Browsers, GNOME's image viewer and Pillow all decode JPEG with
the same library, libjpeg-turbo, and this is now a port of it, agreeing with
it to the bit on every test file and on 32,000 damaged ones. Before, pixels
were within a few levels of it -- invisible -- but some kinds of JPEG came out
wrong or not at all: CMYK files from print work showed false colours,
RGB-coded JPEGs came out in the wrong colours, and arithmetic-coded files
and files sending each colour in a scan of its own were refused. Those now
show as they do elsewhere, and so do lossless JPEGs, from medical and
scientific imaging, which were refused too. It is also faster: a
21-megapixel photograph in 0.76 s rather than 1.42 s, its thumbnail in
0.18 s rather than 0.45 s.

### Why a port rather than a better decoder of our own

The old decoder implemented the standard, with libjpeg's upsampling filter
added (§1306), a floating-point inverse DCT, and tests that allowed 3 levels
a channel. The standard allows that latitude, but what anyone compares a
picture against is what their other programs show, and they all show
libjpeg-turbo. TIFF needed more: everything else in the TIFF port (§1317) is
held to libtiff's exact output, and a JPEG-compressed TIFF is decoded by
libjpeg underneath. And the latitude is only about rounding. What a decoder
does with a *damaged* file -- where it stops, what fills the rest, which
markers it forgives, when it gives up -- is where decoders differ visibly,
and there the only reference is libjpeg's own behaviour.

*Alternatives:* (a) keep the old decoder and its tolerance -- cheapest, and
the rounding differences are invisible, but damaged files would go on
decoding differently, JPEG-in-TIFF could not be held exactly, and the CMYK,
RGB and arithmetic gaps would each need a fix of their own; (b) reproduce
libjpeg-turbo's SIMD arithmetic, which computes the inverse DCT in 16-bit
lanes and is what x86 distributions run -- but it is not one reference
(SSE2, AVX2 and NEON can differ where a value overflows), it would be obscure
to maintain, and it agrees with the C code on every file whose coefficients
fit 16 bits, which is every valid one. Chosen: (c) the C code, operation for
operation -- its 64-bit arithmetic, its truncations to `int`, its 10-bit
range-limit wrap -- so that even garbage decodes to libjpeg's garbage.

### What it is

`gui/imagecodec/src/jpeg/`, each part citing the libjpeg-turbo 3.1.1 file it
transcribes: `source` (the fake end-of-image markers every libjpeg data
source supplies past the end of the data, from which the handling of a
cut-off file follows), `marker` (`jdmarker.c`), `huffman` (`jdhuff.c`,
`jdphuff.c`), `arith` (`jdarith.c`), `coef` (`jdcoefct.c`: the coefficient
store of a multi-scan image, and block smoothing), `idct` (`jidctint.c`'s
accurate integer transform, `jidctred.c`'s reduced ones), `upsample`
(`jdsample.c`), `color` (`jdcolor.c`), and `decompress` (`jdapimin.c`,
`jdapistd.c`, `jdinput.c`, `jdmaster.c`). The interface is libjpeg's --
read the header, choose colour spaces and scale, start, read rows, finish --
and the quantisation and Huffman tables outlive a datastream, as libjpeg's
permanent pool does: TIFF needs both.

The choices libjpeg leaves to its caller are taken, for the crate's own entry
points, as Chrome takes them: RGB out for RGB and YCbCr files;
CMYK and YCCK converted by Chrome's formula for the inverted CMYK Adobe
writes (`c * k / 255`); two components, or five and more, refused, as Chrome
refuses them; at most 100 scans, Chrome's (and libtiff's) progress-monitor
limit; and nothing after the last row read -- a picture whose rows all
decoded is shown whatever follows, where `jpeg_finish_decompress` could still
object to it. A file cut off shows the rows that arrived and then grey, as in
every program built on libjpeg; `dimensions` reads the header as libjpeg
does, so a file whose header libjpeg refuses no longer reports a size (the
old walker reported the first frame's size whatever surrounded it).

One choice is not Chrome's: a greyscale file is decoded as greyscale and made
RGB by the crate, as GNOME's image loader and Pillow do it, where Chrome asks
libjpeg for RGB. For every lossy file the pixels are the same -- libjpeg's
grey-to-RGB conversion only copies -- but libjpeg converts nothing at all in
a lossless image, so Chrome's request makes a lossless greyscale JPEG fail,
and greyscale is the kind lossless JPEG mostly is. *Against:* one more place
where "what Chrome shows" is not the rule, and a browser shows those files
as broken where this shows them; *for:* the free desktop's viewers show
them, and nothing that decodes changes.

Two parts of the old decoder live on: its compact coefficient store for
thumbnails of progressive files (§1305), which keeps exactly the
coefficients libjpeg's reduced transforms read and, for the rest, only
whether each is zero -- all a refinement scan ever asks of a coefficient it
does not produce -- so it stays exact, and a thumbnail of a large progressive
photograph still costs a fraction of its coefficients; and the upsampling
filters (§1306), now chosen as libjpeg chooses them, including its refusal of
sampling ratios that are not whole numbers.

The store is also decoded into the way libjpeg decodes into its coefficient
array: a progressive AC scan's decoder reads and writes each coefficient
where it lies (`coef::StoredBlock`), a DC scan moves the DC alone. The port
first copied every block out and back for every scan, and that copying cost
a large progressive photograph's thumbnail more than its decoding -- slower
than the decoder it replaced, until this; a progressive thumbnail now takes
0.56 s against its 0.70 s.

Old-style JPEG in TIFF (§1317) asked three more things of it, all libjpeg's:
raw output (`raw_data_out`: each component's samples an iMCU row at a time,
which libtiff packs into TIFF's subsampled `YCbCr`), a decompressor that owns
its data and tables (libtiff keeps one session across strip reads), and a
source that fails where libtiff's does -- data run out, a skip, a restart
marker out of step. The last made reading ahead matter: the decoder read the
next iMCU row a row early whatever the upsampler, harmless while running out
could not fail; it now reads it when libjpeg's main controller does -- for
the last row group of this one if an upsampler needs the rows below,
otherwise not before its own first row.

### Lossless JPEG

Ported the same day, in `lossless` (`jdlhuff.c`, `jddiffct.c`,
`jdlossls.c`): all seven predictors, the point transform, samples of 2 to 8
bits handed out as they are (a 6-bit image runs from 0 to 63), any
sampling, interleaved or in scans of their own. libjpeg-turbo's quirks are
kept: it undifferences an iMCU row only once the row is decoded, so a
restart marker inside one -- a component taller than one sample per MCU, in
a scan of its own -- resets the predictor for the iMCU row's first row, not
the row after the marker; data that runs out gives grey; a component no
scan carries fails the decode, because libjpeg's whole-image sample array
is not zeroed and reading an unwritten row of it is an error. A restart
interval must be whole rows of MCUs. Samples wider than 8 bits need
libjpeg's 12- and 16-bit interfaces, and are refused, as 12-bit lossy JPEG
is; arithmetic-coded lossless (`SOF11`) libjpeg-turbo does not implement.

### How it is held

Every JPEG test in the crate now compares exactly; the tolerance is gone
(`tests/common/mod.rs`). The 24x16 reference, every chroma layout at full
size and at a half, a quarter and an eighth against TurboJPEG, the
progressive fixtures and the EXIF orientation fixtures all pass to the bit.
A libjpeg-turbo 3.1.1 oracle (its C build) answered 146 seeds made by
`cjpeg` and Pillow -- baseline at five subsamplings, greyscale, progressive,
arithmetic sequential and progressive, restart markers, RGB, optimised
tables, 16-bit quantisers, separate-scan sequential, CMYK -- all identical
at all four sizes; then 32,000 mutants of them (header fields, segment
lengths, marker codes, markers spliced into entropy data, cuts, bit flips).
The only disagreements, two thumbnails, came from the old header walker the
thumbnail path still used; it now reads the header through the port.

Lossless JPEG has 56 fixtures of its own (`tests/jpeg_lossless.rs`), written
by an encoder in `tests/data/generate_jpeg_lossless.py` that can produce
every layout and the damage that matters, answered by the same libjpeg-turbo
build -- and every undamaged one checked to decode to the picture that went
in, which is what lossless means. Then 256 lossless seeds -- those, `cjpeg
-lossless` at twelve settings, and 140 random layouts -- and 20,000 mutants of
them, without a disagreement.
