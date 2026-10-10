### [F] Video is shown without colour management: HDR looks dim and grey, wide gamut oversaturated -- 2026-10-04

**Status:** OPEN (lane F), in part -- **HDR is done**: PQ and HLG video and
AVIF are shown as Chrome shows them on an ordinary screen, tone mapped by the
light they say they hold, BT.2020's colours carried to sRGB's, and held to
Chrome's own pixels (design-decisions §1378, §1379; `gui/video/yuv/src/managed.rs`).
What remains is below. **Decided 2026-10-09** (F-Q10, §1378): colour
management copies Chrome's -- skcms for pictures' profiles, Chrome's own
handling for video.

**In short:** a video's pictures are turned into pixels by the recipe the
video names (its matrix and range, `gui/video/codec`, §1346). HDR pictures
are then shown as Chrome shows them. Ordinary (SDR) pictures are shown as if
every pixel were sRGB's -- right for HD video, but not for
standard-definition video that says it has BT.601's colours (Chrome adjusts
those, mostly in saturated colours), nor for SDR pictures made for wider
colours (BT.2020 SDR, Display P3), which show with their colours pushed
further than intended; nor for pictures whose colours an ICC profile
describes (iPhone photos in Display P3, cameras' Adobe RGB), which show dull
or oversaturated.

**What is ignored.**

| The picture says | Today | What Chrome does |
|---|---|---|
| Primaries other than BT.709's in SDR video (BT.601's, BT.2020 SDR, P3); transfers other than the sRGB-like ones (gamma 2.2 and 2.8, linear, SMPTE 240M) | shown as BT.709's, by the sRGB curve | converts to the screen's primaries and curve (`gfx::ColorSpace`'s transfer and gamut matrix) |
| An ICC profile (PNG, JPEG, WebP, AVIF, icons) | not applied | converts through skcms |
| A gain map (ISO 21496-1: UltraHDR JPEG, AVIF `tmap`) | the SDR base is shown | applies it for an HDR screen, none for an SDR one |
| An HDR screen (headroom over 0) | every screen taken as sRGB of no headroom | RWTMO's other alternate images, weighted by the screen's headroom |

**Where.** `gui/video/codec/src/colour.rs` (what is resolved),
`gui/video/yuv/src/reformat.rs` (the ordinary conversion),
`gui/imagecodec` (profiles), and the compositor, which draws everything as
sRGB (`gui/compositor`).

**The proper fix.** As `roadmap.md`'s "Colour management" lists it: SDR
video's primaries converted as Chrome converts them (its gamut matrix, the
sRGB curve for BT.709's and BT.2020's transfers); skcms ported for profiles;
and colour management the compositor's to own once screens report their
colours (EDID): the screen's primaries and headroom, a working space for
composition, and each picture's conversion to it. Gain maps follow the
profile work.

**What happens until then.** Ordinary SDR video and pictures, nearly all of
both, are shown correctly; HDR is shown as Chrome shows it on an sRGB screen;
SDR wide-gamut video and profiled pictures show with the wrong saturation.

**And HDR's speed.** The HDR conversion is floating point where the
ordinary one is libyuv's fixed point. Its six table lookups a pixel run
eight pixels at a time with AVX2 (detected at run time, §1373;
`gui/video/yuv/src/managed/avx2.rs`, to the scalar passes' bits), which takes
steps 2 to 5 from 1.7 to 3.6 times faster; a 1080p frame took 40-60 ms on
one thread before (its bands run one to each core, as the ordinary
conversion's do), where 4K at 60 frames a second wants several times less.
What is left: step 1, the Y'CbCr and its chroma in floating point (some two
fifths of a frame now), and the tone map's `exp2f` in its curved middle,
still a pixel at a time (`roadmap.md`, "Colour management").

**SDR primaries are not only wide gamut's.** Probed 2026-10-10 in Chrome
154 (a lossless VP9 frame of colour patches, tagged each way by Matroska's
`Colour`): Chrome converts every primaries but BT.709's to sRGB's --
BT.601's 525-line (NTSC, H.273 code 6) and 625-line (PAL, 5) ones too,
which most standard-definition video says it has -- and the transfers it
does not take for the sRGB curve (gamma 2.2 and 2.8, linear, SMPTE 240M).
BT.709's, BT.601's, BT.2020's and sRGB's transfers all show as the sRGB
curve, unconverted. Today none of those conversions is made: saturated
colours come out up to 77 levels (of 255) from Chrome's.
