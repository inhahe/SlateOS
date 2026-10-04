### [F] Video is shown without colour management: HDR looks dim and grey, wide gamut oversaturated -- 2026-10-04

**Status:** OPEN (lane F) -- a missing subsystem, not a bug in what exists.

**In short:** a video's pictures are turned into pixels by the recipe the
video names (its matrix and range, `gui/video/codec`, design-decisions §1346),
and then shown as if every pixel were in the ordinary range of colours and
brightness a screen shows (sRGB). For nearly all video that is right. For HDR
video (HDR10, HLG -- YouTube's "HDR" videos, many recent films) it is not:
those store far brighter light in a different curve, and shown this way they
look dim and washed out. Video made for the wider colours of BT.2020 also
shows with its colours pushed further than intended. Chrome and mpv convert
both to what the screen can show (tone mapping, gamut mapping); SlateOS does
neither yet. AVIF stills are in the same position (`gui/imagecodec`).

**What is ignored.**

| The stream says | Today | What a colour-managed path does |
|---|---|---|
| Transfer: PQ (16, HDR10) or HLG (18) | not read; the samples are shown as SDR | maps the HDR light to the display's range (tone mapping; mpv's `bt.2390`, Chrome's) |
| Primaries: BT.2020 (9), Display P3 (12), ... | read only where the matrix is derived from them (12, 13) | converts to the display's primaries (gamut mapping) |
| Mastering display, content light level (HDR metadata) | not read | guides the tone mapping |

**Where.** `gui/video/codec/src/colour.rs` (what is resolved),
`gui/video/yuv/src/reformat.rs` (the conversion), and the compositor, which
draws everything as sRGB (`gui/compositor`).

**The proper fix.** Colour management, which is the compositor's to own: a
display's characteristics (its primaries and brightness, from EDID), a
working colour space for composition, and a tone-mapping and gamut-mapping
step for content that declares something else -- applied to video frames,
AVIF and PNG pictures with colour profiles alike. The video API would then
hand on the stream's transfer function and primaries with each frame instead
of dropping them (`matroska::Colour` reads `TransferCharacteristics` and
drops it today, for want of a reader, design-decisions §856).

**What happens until then.** Ordinary (SDR, BT.601/BT.709) video, which is
nearly all of it, is shown correctly. HDR and wide-gamut video plays, with
the wrong brightness and saturation.
