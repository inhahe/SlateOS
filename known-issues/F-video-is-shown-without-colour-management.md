### [F] Video is shown without colour management: HDR looks dim and grey, wide gamut oversaturated -- 2026-10-04

**Status:** OPEN (lane F), in part -- **HDR is done**: PQ and HLG video and
AVIF are shown as Chrome shows them on an ordinary screen, tone mapped by the
light they say they hold, BT.2020's colours carried to sRGB's, and held to
Chrome's own pixels (design-decisions §1378, §1379; `gui/video/yuv/src/hdr.rs`).
What remains is below. **Decided 2026-10-09** (F-Q10, §1378): colour
management copies Chrome's -- skcms for pictures' profiles, Chrome's own
handling for video.

**In short:** a video's pictures are turned into pixels by the recipe the
video names (its matrix and range, `gui/video/codec`, §1346). HDR pictures
are then shown as Chrome shows them. Ordinary (SDR) pictures are shown as if
every pixel were sRGB's -- right for nearly all video, but not for the few
SDR pictures made for wider colours (BT.2020 SDR, Display P3), which show
with their colours pushed further than intended; nor for pictures whose
colours an ICC profile describes (iPhone photos in Display P3, cameras'
Adobe RGB), which show dull or oversaturated.

**What is ignored.**

| The picture says | Today | What Chrome does |
|---|---|---|
| Primaries other than BT.709 in SDR video (BT.2020 SDR, P3) | shown as BT.709's | converts to the screen's primaries (`gfx::ColorSpace`'s gamut matrix) |
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

**And HDR's speed.** The HDR conversion is scalar where the ordinary one is
libyuv's fixed point: some 40-60 ms for a 1080p frame on one thread (its
bands run one to each core, as the ordinary conversion's do), where 4K at 60
frames a second wants several times less. Its arithmetic passes already run
four pixels at a time; the rest is six table lookups a pixel, which AVX2's
gathers (detected at run time, §1373) would take eight at a time
(`roadmap.md`, "Colour management").
