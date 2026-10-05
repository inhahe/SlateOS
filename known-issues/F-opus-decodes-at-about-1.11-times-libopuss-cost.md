### [F] Opus decodes at about 1.11 times libopus's cost -- 2026-10-04

**Status:** OPEN — lane F's; tech debt, not a bug. Mostly paid on
2026-10-04 (it was 1.36 times).

**In short:** the Opus decoder (`gui/video/opus`) does about a tenth more
work than libopus's own C to decode the same sound -- the same samples to
the bit, just a little more slowly: 4.46 billion instructions to libopus's
4.03 on its benchmark (every test stream at 48 kHz, 151 s of sound;
callgrind, so other load does not move the counts), which is 391 times
faster than real time against libopus's 426. Nothing a user does is held
up by it.

**Done** (design-decisions §1350): no allocation or clearing per frame --
the band decoder's folding copies, the CELT spectrum, the folding buffer
and the inverse transform's input kept between frames or on the stack,
SILK's and the multistream decoder's buffers likewise; the de-emphasis at
full rate straight through both channels at once (libopus's
`deemphasis_stereo_simple`); the ambisonics matrix, the comb filter, the
2:3 and 3:4 resampler and the FFT's radix-2 and last radix-4 stages
written so that the compiler drops their bounds checks. The ambisonics
matrix and the comb filter now cost what libopus's do.

**Where the rest is** (callgrind, port against libopus, millions of
instructions):

| | port | libopus |
|---|---|---|
| inverse MDCT and its FFT (`CeltDecoder::synthesis`) | 1443 | about 1243 |
| de-emphasis | 264 | 223 |
| SILK's 2:3 and 3:4 resampler (`iir_fir`) | 351 | 322 |

**The proper fix, the rest:**
1. The FFT's radix-3, radix-5 and general radix-4 butterflies, and the
   MDCT's rotations, still index with bounds checks. A first attempt at
   the radix-4 one through iterators (`step_by` over the twiddles) cost
   *more* instructions than the indexed loop and was reverted; what works
   is likely the arrays-of-fixed-size form the degenerate stage uses, with
   the twiddle index arithmetic hoisted. Profile with line tables
   (`debug = 1`) first: the whole transform is inlined into one function,
   so callgrind cannot say which part costs.
2. De-emphasis at reduced rates still filters through `chunks`; and its
   full-rate loop is a little behind libopus's.
3. Re-measure with `tests/bench.rs` under callgrind after each, the digests
   (`tests/streams.rs`) holding every change to libopus's samples.
