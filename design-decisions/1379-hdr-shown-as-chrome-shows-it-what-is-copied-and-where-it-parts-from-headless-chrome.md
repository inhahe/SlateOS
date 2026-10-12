## 1379. HDR shown as Chrome shows it: what is copied, and where it parts from headless Chrome

**Date:** 2026-10-10
**Lane:** F
**Decided by:** Claude (operator-approved scope) — the operator chose to copy
Chrome's colour management (§1378, F-Q10 A); the specific calls below, each
made by measuring Chrome itself, are Claude's.

**In short:** HDR video and HDR AVIF pictures are now shown the way Chrome
shows them on an ordinary screen -- bright highlights rolled off, ordinary
brightness kept, BT.2020's colours carried into the screen's -- and are
checked against screenshots of Chrome itself, not against a description of
it. In two places headless Chrome turned out to do something a real Chrome
on a real graphics card does not (it cuts 10-bit video to 8 bits on the way
to the screen); those are not copied.

**Context.** §1378 settled *whose* handling to reproduce. Reproducing it
left choices the operator's answer did not make: which parts of Chrome's
pipeline are Chrome's on purpose and which are accidents of the machine it
runs on; how chroma at half resolution is brought up (Chrome never says --
its GPU does it); what an HDR stream that leaves its colour unsaid is taken
to be; and how much arithmetic precision is enough.

**Decision.**

- **The conversion** (`gui/video/yuv/src/hdr.rs`) is Skia's, transcribed:
  `SkColorSpaceXformSteps` (skcms's PQish curve, or its HLGish curve and
  BT.2100's OOTF, into linear BT.2020 with 1.0 at 203 cd/m2, the scale folded
  into the gamut matrix as Skia folds it), the tone map Skia adds to a PQ or
  HLG picture that brings none (skhdr's adaptive global tone map, RWTMO,
  alternate image 0 for a screen of no headroom; the content's peak from
  MaxCLL, else the mastering display's, else 1000 cd/m2; control points
  computed in single precision and rounded to half precision as Skia's shader
  reads them), and Skia's gamut matrices to sRGB. The curves are tables, made
  once per transfer, erring by under a ten-thousandth of an 8-bit code, and
  the last rounding is exact: against a double-precision transcription the
  codes agree in all but under one channel in ten thousand.
- **Chroma at half resolution is brought up as a GPU samples it**:
  bilinear, centred, clamped at the edges -- 9:3:3:1 of the four nearest
  samples in floating point, which is libavif's slow path to the letter. For
  4:2:0 HDR pictures of noise, Chrome's pixels agree with this in 98.8% of
  channels (an AVIF), and in 98.8% (a video, once the cut below is made), the
  rest by one; with libyuv's integer upsampling, which ordinary pictures keep
  (§1344), in 76% and 40%.
- **Not copied: headless Chrome's cut of 10-bit 4:2:0 video to 8 bits.**
  Its video pixels match only once the samples are cut to 8 bits -- its
  4:4:4 video does not do it, its AVIF does not, and a GPU's 10-bit texture
  path would not. Copying it would band HDR's darks for no reason of
  Chrome's own design.
- **An HDR stream's unsaid colour** is guessed as Chrome's media stack
  guesses it (`VideoColorSpace::GuessGfxColorSpace`): a transfer of BT.2020's
  makes the unsaid matrix and primaries BT.2020's, unless either says
  BT.709's (`gui/video/codec/src/colour.rs`). Ordinary video keeps mpv's
  guess (§1346). An AVIF's unspecified code points are MIAF's defaults, and a
  code point Chrome has no name for, or an ICC profile, makes it an ordinary
  picture -- as Chrome's AVIF decoder takes them
  (`gui/imagecodec/src/avif/convert.rs`).
- **The light**: video's per frame, the bitstream's kind by kind over the
  file's (as FFmpeg gives Chrome); an AVIF's from its `clli` box alone, the
  one HDR metadata Chrome's AVIF decoder reads.
- **The screen** is an sRGB one of no headroom until screens report their
  own (§1378); then alternate images 1 and the baseline apply as Skia
  weights them.

**Rationale.** The reference is Chrome as people see it, which is Chrome on
a GPU; where headless Chrome's software path differs from that, matching the
headless screenshots would copy a defect. Measuring rather than reading
settled the upsampling: Chrome's source leaves it to the GPU, and two
plausible readings (libyuv's, the one ordinary pictures use; and centred
bilinear) differ by up to 44 levels on chroma-rich content.

**Alternatives considered.**
- *libyuv's integer upsampling for HDR video*, as ordinary video uses: one
  upsampling for all video. Rejected: Chrome's pixels say otherwise.
- *Matching headless Chrome's video bit for bit*, 8-bit cut included:
  the highest score against the screenshots, and worse pictures.
- *Per-pixel `pow` and `exp`*, as Skia's CPU path does: the same answers
  within its own approximations, and three to four times the time; libm's
  `powf` alone cost more than the rest of a pixel.

**Checked by.** `gui/video/yuv` (Chrome 154's own pixels for 24 PQ and 16
HLG patches, exact; Skia's control points; the double-precision
transcription; 20 mutations); `gui/imagecodec/tests/avif_hdr.rs` (headless
Chrome's screenshots of four AVIFs, one with a `clli` the generator checks
Chrome reads: exact for the 4:4:4 patches, within one in under 2% of
channels for 4:2:0 noise); `gui/video/codec/tests/hdr.rs` (nine HDR streams'
first frames against the same arithmetic in double precision, from ffmpeg's
decoding).

**Where it lives.** `gui/video/yuv/src/hdr.rs`, `reformat::chroma_of_row`;
`gui/video/codec/src/picture.rs` (the light, the bands), `colour.rs` (the
guess); `gui/imagecodec/src/avif/convert.rs` (which AVIFs are HDR), the
`clli` property through `setup.rs` and `decode.rs`. The references:
`gui/video/codec/tests/data/chrome_hdr.py`,
`gui/imagecodec/tests/data/generate_avif_hdr.py`.

**How to reverse.** The upsampling is one function (`hdr::convert`'s call
of `chroma_of_row`); the guess one branch of `colour::resolve`; the AVIF
rules `hdr_of`. Each is held by its tests and mutation rows, which say what
changes.
