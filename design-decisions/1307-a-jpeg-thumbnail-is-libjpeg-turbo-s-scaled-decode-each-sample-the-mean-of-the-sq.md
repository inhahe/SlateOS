## 1307. A JPEG thumbnail is libjpeg-turbo's scaled decode: each sample the mean of the square it stands for, and 4:2:0 colour reconstructed at the output's size

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous). Mechanism inside `gui/imagecodec`; the
trade it makes is memory in a progressive thumbnail, recorded below.
**Superseded by §1318** the same day: scaled decoding is now libjpeg-turbo's
own reduced transforms (`jidctred.c`), bit for bit, and the progressive
thumbnail's store keeps what they read.

**In short:** a JPEG thumbnail is made by decoding the photograph directly at
a half, a quarter or an eighth of its size, which is far cheaper than decoding
it whole and shrinking it. This decoder's way of doing that was its own: its
reduced picture sat half an original pixel off the full one, and its colour
was stretched up from a plane a quarter the size. It now does exactly what
libjpeg-turbo does — the library under every mainstream browser, image
library and desktop — so a thumbnail here is the one every other program
would make of the same file, and like the full-size decode it can be tested
against that.

### What changed

1. **The reduced transform averages** (`idct_scaled`). Each reduced sample is
   now the mean of the full transform's samples over the square it stands for,
   which is what libjpeg-turbo's `jidctred.c` computes — its header says so,
   and a probe confirmed it: TurboJPEG's scaled greyscale decode is its full
   decode box-averaged, to within 0.75 of a level. The mean of a basis function
   over a group of samples is a number, so the reduced transform is the full
   one with averaged weights (`Basis::reduced`); the frequencies whose averages
   are all zero (the fourth over pairs, the even ones over fours, all but the
   DC over the whole block) get weights of exactly zero. The old transform
   used only the top-left `n` x `n` coefficients and sampled the basis at whole
   positions, half a source pixel from the centres of the squares it stood for.
2. **4:2:0 colour is reconstructed at the output's size** (`component_block`,
   libjpeg's `jdmaster.c` rule). A component sampled coarsely enough in both
   directions has its blocks transformed at twice the picture's block size —
   four times, for colour quartered both ways — so at half scale 4:2:0 chroma
   gets a full 8x8 transform and needs no upsampling at all. Colour halved one
   way only (4:2:2, 4:4:0) cannot be, and is upsampled as at full size (§1306).
3. **A progressive thumbnail keeps what its transform reads** (`Kept`): 7
   coefficients a side at half scale, 5 at a quarter, the DC alone at an
   eighth, per plane — since chroma may now be transformed at a larger size
   than luma. The zero weights are exact, so a dropped coefficient changes no
   bit of the output, and the progressive scaled decode is still its baseline
   twin's bit for bit.

### The choices with two sides

1. **libjpeg-turbo's averaging, not IJG libjpeg 9's truncation.** The other
   well-known reduction transforms only the low `n` x `n` coefficients,
   sampled at the right centres: a sharper picture (it keeps every frequency
   the smaller size can show, at full strength), with some ringing at edges,
   and a progressive thumbnail's store stays at `n` x `n`. *For averaging:* it
   is what libjpeg-turbo does, so the thumbnail is the one users see in every
   other program, and there is a reference to hold it to; `decode_scaled`
   already box-filters after the transform, so the whole path is now one
   filter; no ringing. *Against:* softer; and a progressive thumbnail keeps
   more — 49 coefficients a block at half scale instead of 16, 25 at a quarter
   instead of 4 (an eighth is unchanged, the DC alone).
2. **4:2:0 colour at twice the block size.** *For:* the colour is resolved by
   the transform at the thumbnail's own resolution, which is a better picture
   than upsampling a plane a quarter the size, and it is libjpeg-turbo's.
   *Against:* at an eighth of the size a progressive 4:2:0 thumbnail keeps 25
   coefficients per chroma block instead of 1: 156 bytes a 16x16 MCU instead
   of 60, or for a 24-megapixel photograph about 15 MB instead of 6 — still a
   twentieth of what the full picture's store needs, and far inside
   `gui/thumbs`' budget of 16 bytes per source pixel. A baseline decode holds
   no coefficients and pays nothing.

### How it is held

- `jpeg.rs` unit tests: every reduced sample is the mean of the full
  transform's samples over its square (2000 random blocks at all three scales);
  the coefficients a reduced transform ignores change no bit of its output;
  each layout gets libjpeg's per-component size.
- `tests/jpeg_sampling.rs`: every fixture — every colour layout, and the narrow
  planes — at a half, a quarter and an eighth of its size against TurboJPEG's
  own scaled decode, generated through `simplejpeg`.
- `tests/jpeg_progressive.rs`: each progressive file's scaled decode is still
  its baseline twin's at five sizes; the byte-budget test recounts what a
  progressive thumbnail holds.

### Measured

Against TurboJPEG's own scaled decode, every fixture at every scale is now
within 2 levels (mean under 0.06 on every fixture big enough to average); the
previous decoder was up to 157 levels off, with a mean of 17 on 4:2:0 at half
scale.

Speed, release, a synthetic 4000x5333 photograph at quality 90, the least of
nine runs taken alternately with the previous decoder on a busy machine:
eighth scale -- every thumbnail of a photograph that size -- unchanged (4:2:0
0.31 s both; 4:2:2 0.39 s against 0.42 s, inside the noise); half scale much
faster (4:2:0 1.23 s to 0.70 s, 4:2:2 1.71 s to 0.98 s), since the old
transform skipped no zeros and 4:2:0 chroma now needs no upsampling; quarter
scale a little slower by the least run (4:2:0 0.40 s to 0.43 s, 4:2:2 0.54 s
to 0.59 s) and about level at the quarter mark of the runs (0.45 to 0.47, 0.69
to 0.69) -- the averages weigh five frequencies a side where truncation
weighed two. It took three rounds to get there: the first averaging
transform, walking a runtime list of frequencies, made the eighth-scale 4:2:0
thumbnail 37% slower. An eighth-scale block is now its DC outright
(`flat_block`), each size is a const-generic kernel that unrolls, and
mirrored outputs share an even and an odd sum (`reduced_transform`).
