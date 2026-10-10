### [F] Video is shown without colour management: HDR looks dim and grey, wide gamut oversaturated -- 2026-10-04

**Status:** OPEN (lane F), in part -- **HDR is done**: PQ and HLG video and
AVIF are shown as Chrome shows them on an ordinary screen, tone mapped by the
light they say they hold, BT.2020's colours carried to sRGB's, and held to
Chrome's own pixels (design-decisions §1378, §1379; `gui/video/yuv/src/managed.rs`).
**So is SDR video that says its colour whole** (§1381): BT.601's primaries,
BT.2020 SDR, Display P3, and the curves Chrome converts, as Chrome converts
them, held to Chrome's pixels (`gui/video/codec/tests/sdr.rs`); **and SDR
AVIF** whose `nclx` names them (`gui/imagecodec/tests/avif_sdr.rs`). What
remains is below. **Decided 2026-10-09** (F-Q10, §1378): colour management copies
Chrome's -- skcms for pictures' profiles, Chrome's own handling for video.

**In short:** a video's pictures are turned into pixels by the recipe the
video names (its matrix and range, `gui/video/codec`, §1346). HDR pictures,
and SDR video and AVIF that name other colours than an ordinary screen's,
are then shown as Chrome shows them. What is left: pictures whose colours
an ICC profile describes (iPhone photos in Display P3, cameras' Adobe RGB),
which show dull or oversaturated; gain maps; HDR screens; and what to do
with video that does not fully say its colours (`open-questions/F-Q11.md`).

**What is ignored.**

| The picture says | Today | What Chrome does |
|---|---|---|
| An ICC profile (PNG, JPEG, WebP, AVIF, icons) | not applied | converts through skcms |
| A gain map (ISO 21496-1: UltraHDR JPEG, AVIF `tmap`) | the SDR base is shown | applies it for an HDR screen, none for an SDR one |
| An HDR screen (headroom over 0) | every screen taken as sRGB of no headroom | RWTMO's other alternate images, weighted by the screen's headroom |
| Video whose colour is said in pieces or not at all | each part used where said, the rest guessed as players guess (§1346); unconverted | BT.601 at the studio range, unconverted (F-Q11) |

**Where.** `gui/video/codec/src/colour.rs` (what is resolved),
`gui/video/yuv/src/managed.rs` (Chrome's conversion),
`gui/imagecodec` (AVIF's colour, profiles), and the compositor, which draws
everything as sRGB (`gui/compositor`).

**The proper fix.** As `roadmap.md`'s "Colour management" lists it: skcms
ported for profiles; and colour management the compositor's to own once
screens report their colours (EDID): the screen's primaries and headroom,
a working space for composition, and each picture's conversion to it. Gain
maps follow the profile work.

**What happens until then.** Ordinary video and pictures, nearly all of
both, are shown correctly; HDR, fully described SDR video and SDR AVIF are
shown as Chrome shows them on an sRGB screen; profiled pictures show with
the wrong saturation.

**And HDR's speed.** The HDR conversion is floating point where the
ordinary one is libyuv's fixed point. Its six table lookups a pixel run
eight pixels at a time with AVX2 (detected at run time, §1373;
`gui/video/yuv/src/managed/avx2.rs`, to the scalar passes' bits), which takes
steps 2 to 5 from 1.7 to 3.6 times faster; a 1080p frame took 40-60 ms on
one thread before (its bands run one to each core, as the ordinary
conversion's do), where 4K at 60 frames a second wants several times less.
What is left: step 1, the Y'CbCr and its chroma in floating point (some two
fifths of a frame now), and the tone map's `exp2f` in its curved middle,
still a pixel at a time (`roadmap.md`, "Colour management"). SDR pictures
Chrome converts take the same path, less the tone map.
