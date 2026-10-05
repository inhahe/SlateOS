## 1306. JPEG colour is brought back up with libjpeg's own filter, rounding and all

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous). It reverses a judgment recorded in the
decoder's own comments ("interpolating would be a better picture than the file
contains"), which was also Claude's; the operator has ruled on neither.
**Superseded by §1318** the same day: the filter lives on, chosen as libjpeg
chooses it, and the 3-level tolerance is gone -- the whole decoder is now
libjpeg-turbo's.

**In short:** most photographs store their colour at half resolution, so a
decoder has to stretch it back to full size. This one used to repeat each
colour sample across the pixels it covers, where every other program — web
browsers, image libraries, desktops, nearly all of them built on the same
library, libjpeg — blends each sample with its neighbours. The difference
showed as stepped, blocky fringes wherever colour changes sharply: up to 105
levels (of 255) away from what any other program shows for the same file.
It now blends them exactly as libjpeg does, and the same files come out within
3 levels of it — the rounding any two correct decoders differ by.

### What it does

`gui/imagecodec/src/jpeg/upsample.rs`, transcribed from libjpeg-turbo's
`jdsample.c`. Colour halved across (4:2:2), down (4:4:0) or both (4:2:0) is
brought back with libjpeg's "fancy" triangle filter: each output sample is
three quarters the input it lies nearer and a quarter the next one out, in
each direction the plane was halved. The two outputs of each input round with
different biases (`+1`/`+2`, and `+8`/`+7` both ways), the outermost sample
stands in for its missing neighbour at every edge, and the filter reads only
the plane's real samples — never the padding that fills out the last block,
which is decoded data from outside the picture. Every other ratio (4:1:1's
quarter, say) repeats each sample, as libjpeg does. Baseline, progressive and
scaled decodes all go through it, since they share `to_pixels`.

### The choices with two sides

1. **Blend, where the old comment said repeat.** Its argument: the file holds
   a quarter of the colour, and interpolating shows a picture better than the
   file contains. *For blending:* the encoder made each colour sample by
   averaging a 2x2 patch, and the smooth reconstruction is the better estimate
   of what it averaged — repeating is not "what the file contains" either, it
   is a different guess with steps in it. And for a JPEG, "correct" in practice
   means what everything else shows: a photograph whose colour edges look
   different here from every browser is a defect report waiting to be filed.
   *Against:* a little more work per pixel (measured below).
2. **libjpeg's filter exactly, not merely a filter like it** — its rounding
   biases, and its choice of when *not* to filter: a plane only one or two
   samples wide (`downsampled_width > 2`), any ratio but two, and an
   eighth-scale decode. *For:* the filter can then be held to exactness, not a
   tolerance — a unit test compares it with a literal transcription of
   libjpeg's pointer-walking loops over every plane from 3x1 to 19x7, with
   garbage in the padding — and the end-to-end comparison has only IDCT
   rounding to allow for. *Against:* the eighth-scale exception is libjpeg's
   implementation limit (its buffer controller cannot supply the neighbouring
   rows at that size), not a judgment about quality, and it is inherited
   anyway. It costs nothing visible: an eighth-scale decode is a thumbnail,
   box-filtered after.
3. **The reference tolerance is 3 levels, from 2.** Each decoder rounds its
   inverse DCT its own way, so a luma and a chroma sample may each come out a
   level apart, and the colour conversion scales chroma by up to 1.772 before
   both round again: one plus 1.772 rounds to 3. It turns up in a few channels
   in ten thousand. The mean is held under 0.10 on every fixture of a thousand
   pixels or more (all are under 0.05); the 36-pixel narrow fixtures are too
   few for a mean to mean anything, and are there for the filter's edge.

### How it is held

- `upsample.rs`'s unit tests: every filter against the transcription above; a
  flat plane stays flat through every filter at eight values (the test that a
  wrong bias fails); the weights by hand; narrow planes repeat; the choice
  table for every layout; odd sizes keep the first of the last output pair.
- `tests/jpeg_sampling.rs`: every colour layout against Pillow's decode
  (libjpeg-turbo): 4:4:4, 4:2:2, 4:2:0 (three ways), greyscale, 4:4:0 and 4:1:1
  written by TurboJPEG itself (Pillow cannot write them), and two planes two
  samples wide and one three wide — the edge of the filter. Filtering the
  narrow ones, or repeating the slim one, puts 41 to 72 of their channels 7 or
  more levels off; the old repetition put jpeg422 105 levels off.
- `tests/jpeg_progressive.rs`: the progressive reference comparison now covers
  every fixture, subsampled ones included (§1305 had to leave them out).

### Measured

Release build, a synthetic 4000x5333 photograph at quality 90, the least of
nine runs taken alternately with the previous decoder on a machine that was
busy with a boot test: the whole 4:2:0 picture 1.07 s before, 0.97 s after --
faster, because luma no longer goes through the per-pixel column lookup the
old repetition used for every plane, which pays for the filter; 4:2:2 1.26 s
before, 1.31 s after; 128-pixel thumbnails unchanged at about 0.32 s, since an
eighth-scale decode is not filtered. The first cut of the filter, a neighbour
iterator that asked at every sample whether it was at an edge, cost 4:2:2 about
a tenth; `in_pairs`, which does the two edge samples on their own and the rest
as three shifted slices zipped together, brought that back to noise.
