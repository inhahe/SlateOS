### [F] Opus decodes at about 1.36 times libopus's cost -- 2026-10-04

**Status:** OPEN — lane F's; tech debt, not a bug.

**In short:** the Opus decoder (`gui/video/opus`) does about a third more
work than libopus's own C to decode the same sound -- the same samples to
the bit, just more slowly: 5.48 billion instructions to libopus's 4.03 on
its benchmark (every test stream at 48 kHz, 151 s of sound; callgrind, so
other load does not move the counts). Decoding is still hundreds of times
faster than real time, so nothing a user does is held up; it is a cost in
battery and in what else the processor could be doing.

**Where it is** (callgrind, port against libopus, millions of
instructions):

| | port | libopus |
|---|---|---|
| inverse MDCT and its FFT (`CeltDecoder::synthesis`) | 1448 | 1243 |
| de-emphasis | 455 | 223 |
| the ambisonics demixing matrix (`ProjectionDecoder::decode`) | 445 | 192 |
| SILK's 2:3 and 3:4 resampler (`iir_fir`) | 402 | 322 |
| the pitch post-filter (`comb_filter`) | 337 | 249 |
| libc: `memset`, `calloc`, `malloc`, `free` | about 360 | about 0 |

**The proper fix:**
1. **No allocation per frame.** `decode_core` takes four `Vec`s a SILK
   frame, the band decoder copies each band's folding source into a fresh
   one, `silk::SilkDecoder::decode` two more, a multistream call one, and
   CELT's `x`, `freq` and `freq2` one each a frame; libopus puts all of these
   on its stack, uninitialised. Keep them as scratch in the decoders (C
   never reads one before writing it, so reuse needs no clearing -- except
   where C clears explicitly).
2. **The loops whose bounds checks stay**: de-emphasis, the matrix, the
   comb filter, the FFT's butterflies -- written over slices of known
   length, or with the index arithmetic hoisted, so that the compiler can
   drop the checks and vectorise as it does for C.
3. Re-measure with `tests/bench.rs` under callgrind after each, the digests
   (`tests/streams.rs`) holding every change to libopus's samples.
