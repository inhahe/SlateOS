## 1346. A video's frames are coloured as AVIF stills are, by the stream's own colour -- and, where nothing says, by players' guess from the size

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** a video stores its pictures as brightness plus two colour
differences (YUV), and turning that back into the red, green and blue a screen
shows needs to know which recipe the video was made with. Most video says
which, in the stream itself or in the file around it; much does not. SlateOS's
video API (`gui/video/codec`, the one call a player makes for a file's frames)
uses what the stream says, then what the file says, and where neither says
anything it guesses from the picture's size the way video players do -- HD
pictures were made with the HD recipe (BT.709), smaller ones with the older
one (BT.601). The arithmetic is exactly what turns an AVIF picture into pixels
(`gui/video/yuv`), so a frame of video and a still of the same picture look
the same.

**What was decided.**
- *The arithmetic* is libavif's (`yuv::reformat`): libyuv's fixed point
  where libyuv has constants (BT.601, BT.709, BT.2020, each at both ranges),
  libavif's floating point for the rest (SMPTE 240M, FCC, YCgCo, RGB stored
  as G, B, R), chroma upsampled bilinearly. Lane E asked for exactly this
  (`requests/f-e-vp9-video-decodes-now.md`, step 2). Held to libavif 1.3.0
  itself, frame by frame, on fifteen fixtures covering every path
  (`gui/video/codec/tests/frames.rs`).
- *Whose word*: the bitstream's first (VP9's colour space and range bit;
  AV1's sequence header), then the Matroska track's `Colour` element, then the
  guess. Unspecified and reserved values say nothing.
- *The guess* is mpv's (`mp_csp_guess_colorspace`, `mp_csp_guess_primaries`;
  VLC and Kodi agree): 1280 wide or more, or taller than 576, is BT.709;
  anything smaller is BT.601, with the 625-line primaries at a height of 576
  and the 525-line ones at 480 or 488. Range unsaid is the studio range.

**Alternatives.**

| | For | Against |
|---|---|---|
| Stream, then file, then mpv's size guess (chosen) | right for untagged HD video, which is nearly always BT.709; what mpv, VLC and Kodi show | a guess can be wrong for an odd file |
| libavif's fallback: BT.601 for anything unsaid | one rule for stills and video | wrong for most untagged HD video: reds and greens visibly off |
| FFmpeg's order: the bitstream's word even when it is "unspecified", ignoring the file's | what `ffmpeg` writes into a frame | throws away a file that says BT.709 around a VP9 stream that says nothing (`vp9_cropped.mkv` is such a file); players then guess anyway |

**Smaller choices made with it.**
- *Crop* (`PixelCrop`) is applied to the converted frame: the pixels inside
  are exactly those of the uncropped picture. FFmpeg crops the YUV planes and
  rounds the left crop to keep its buffers aligned, so it may show a few
  columns more; the file's rectangle is what is shown here.
- *Alpha* comes from `BlockAdditional` 1 only where the track says
  `AlphaMode` 1 (RFC 9559, Chrome). An alpha packet that does not decode
  leaves its picture opaque rather than dropping it.
- *4:4:0* (VP9 profiles 1 and 3), which neither libyuv nor libavif converts,
  has its chroma doubled down the columns as libyuv doubles 4:2:0's (the
  first row alone, then 3:1 and 1:3), then converts as 4:4:4. No reference
  decoder's pixels to hold it to: the rule is libyuv's vertical half.
- *The display size* keeps the frame's height and takes the width the file's
  `DisplayWidth` : `DisplayHeight` gives it, as FFmpeg reads those two (a
  pixel's shape).

**What it does not do.** Colour management: the transfer function and
primaries are not acted on, so HDR and wide-gamut video are not mapped to the
display (`known-issues/F-video-is-shown-without-colour-management.md`).

**How to reverse.** The rule is one function, `colour::resolve`; the
fixtures' table in `tests/data/generate_fixtures.py` states it row by row and
checks each row against the stream before making answers.

**Revisit when** colour management arrives (the transfer function then
matters), or a real file shows the guess wrong.
