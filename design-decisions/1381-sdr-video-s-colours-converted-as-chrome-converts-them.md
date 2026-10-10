## 1381. SDR video's colours converted as Chrome converts them, when the video says its colour whole

**Date:** 2026-10-10
**Lane:** F
**Decided by:** Claude (operator-approved scope) — the operator chose to copy
Chrome's colour management (§1378, F-Q10 A); how Chrome does it for
ordinary video was measured, and the calls below are Claude's. What to do
with video that does not say its colour whole is asked in
`open-questions/F-Q11.md`.

**In short:** Chrome adjusts the colours of ordinary (not HDR) video
whenever the video says it was made for other colours than an ordinary
screen's -- not only wide-gamut video (BT.2020, Display P3) but also the
standard-definition video that says it uses BT.601's colours, and video
that names an unusual brightness curve -- and AVIF pictures likewise.
SlateOS now does the same, to within one shade of Chrome's own pixels. For
video it does so only where the video says all of its colour in one place,
which is when Chrome does it.

**What Chrome does** (probed in Chrome 154 with tagged test videos, and read
in its source):

- **The conversion.** Skia's colour conversion, as for HDR less the tone
  map: Y'CbCr to R'G'B' in floating point and clamped; the transfer's curve
  as `gfx::ColorSpace::GetTransferFunction` names it -- the sRGB curve for
  BT.709's, BT.601's, BT.2020's and sRGB's transfers, a power of 2.2 or 2.8,
  linear, SMPTE 240M's, ST 428-1's, and nothing at all for the four it has
  no curve for (log, IEC 61966-2-4, BT.1361); the primaries to sRGB's
  (skcms, Bradford, Skia's named gamuts); the sRGB curve's inverse; 8 bits.
  Every primaries but BT.709's are converted, BT.601's two included -- by up
  to 69 levels in saturated colours -- and every curve but the sRGB one.
- **When.** Only for a colour said whole -- primaries, transfer, matrix and
  range, each a code Chrome names (`gfx::ColorSpace::IsValid`) -- in one
  place: Chrome's libvpx decoder (VP8, VP9) takes the file's if the file
  says all four, else the bitstream's, reading VP9's one colour-space field
  as all four (BT.601 as SMPTE 170M's primaries, curve and matrix; BT.2020
  with its curve for 10 and 12 bits, BT.709's for 8); its dav1d decoder
  (AV1) takes the sequence header's, else the file's. Matroska's Range
  element is needed: a `Colour` without it is not whole, and its range 3
  ("derived") is the studio range. A colour not whole anywhere Chrome shows
  as BT.601 at the studio range, unconverted.

**Decision.**

- `gui/video/yuv/src/managed.rs` converts SDR pictures too: `Transfer` gains
  `Sdr(SdrCurve)` with Chrome's curve for each transfer, an SDR `Conversion`
  goes straight to sRGB's primaries in Skia's one step, and the rest is the
  HDR pipeline (§1379, §1380). Held to Chrome's pixels for 2048 codes under
  seven taggings (`tests/data/chrome_sdr.py`): within one level everywhere,
  97.8-99.4% of channels exactly.
- `gui/video/codec/src/colour.rs` reads a colour said whole as Chrome does
  (`ColourHint::whole`, `Colour::whole`), each decoder asking the place
  Chrome's asks first; `Colour::converted` says which pictures take
  Chrome's conversion. Held to Chrome's screenshots of twelve fixtures
  (`tests/sdr.rs`, `tests/data/generate_sdr_fixtures.py`): which place's
  colour stands, VP9's field read whole, the conversions of BT.601's,
  BT.2020's, Display P3's and SMPTE 240M's primaries and of a power of 2.2,
  on 4:4:4 and 4:2:0 pictures -- within one level, 98.4-99.4% exactly.
- A colour said whole is taken whole: where the file says all four and the
  bitstream something else, the place Chrome asks first stands entire --
  its matrix and range too, not only its primaries. This changes the
  matrix only for files whose two places disagree.
- A colour not said whole keeps the players' handling (§1346): each part
  from wherever it is said, the rest guessed, nothing converted (HDR
  aside, which is tone mapped however its colour was found). Chrome's
  BT.601-for-everything is not copied until the operator says so (F-Q11).
- Chrome's residue -- its own arithmetic rounds a few values within a
  twentieth of a level of one half the other way -- is not chased: no order
  of single-precision operations tried reproduced it, and a pixel one
  shade off in 1-2% of channels is below what any screen shows.

**Alternatives.**

| | For | Against |
|---|---|---|
| Convert as Chrome, when said whole (chosen) | Chrome's pixels for every fully described video -- what the big video sites serve, which says its colour, and standard-definition video that says BT.601 | standard-definition video saying BT.601 now looks slightly different from before (as in Chrome) |
| Convert only wide gamut (BT.2020, P3), leave BT.601 alone | BT.601's primaries are close to BT.709's | not what Chrome does: up to 69 levels apart in saturated colours |
| Convert whatever part is said, however | uses every scrap | further from Chrome for partly tagged files, which Chrome leaves alone |

- **AVIF stills the same.** Chrome's AVIF decoder reads the `nclx` box with
  MIAF's defaults (`AVIFImageDecoder::GetColorSpace`) and converts what it
  names as it does video -- Chrome decodes an AVIF to its Y'CbCr planes and
  converts them on the GPU, in floating point (probed: its pixels are the
  floating-point model's under eight taggings, and not libavif's 8-bit
  RGB's). `gui/imagecodec` converts an SDR AVIF whose primaries or curve
  are not sRGB's through `yuv::managed` too, held to Chrome's screenshots
  (`tests/avif_sdr.rs`): within one level, 98.4-99.1% exactly.

**Where it lives.** `gui/video/yuv/src/managed.rs` (`Transfer`,
`SdrCurve`, `Conversion`), `gui/video/codec/src/colour.rs` (`ColourHint::whole`,
`Colour::whole`, `Colour::converted`, `resolve`'s `Prefer`),
`gui/video/codec/src/picture.rs` (`Way::Managed`),
`gui/imagecodec/src/avif/convert.rs` (`managed_of`).

**Found on the way.** An ordinary picture -- one Chrome does not convert --
is converted here by libavif's arithmetic (libyuv's fixed point, §1346),
but by Chrome's GPU in floating point, video and AVIF alike; libyuv's
capped blue weight puts saturated blues and yellows up to 15 levels from
Chrome's. Recorded, and the fix planned:
`known-issues/F-ordinary-video-is-libyuv-s-arithmetic-not-chrome-s.md`.

**Revisit when** the operator answers F-Q11, or when screens report their
colours (the conversion's target becomes the screen's, not sRGB's).
