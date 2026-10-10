### [F] Ordinary video is converted by libyuv's arithmetic, not Chrome's: saturated blues and yellows up to 15 levels off -- 2026-10-10

**Status:** OPEN (lane F). AVIF stills too: Chrome decodes an AVIF to its
Y'CbCr planes and converts them on the GPU as it does video (probed
2026-10-10: Chrome's AVIF pixels are the floating-point model's to within
one level under eight taggings, and libavif's 8-bit RGB's only in two
thirds of the codes), while `gui/imagecodec` converts an ordinary AVIF by
libavif's own arithmetic, libyuv's.

**In short:** a video's pixels are stored as brightness plus two colour
differences, and turning them into red, green and blue is a small sum per
pixel. SlateOS does that sum the way libavif and libyuv do -- in whole
numbers, to match how AVIF pictures are converted -- while Chrome, drawing
video on a graphics card, does it in floating point with the exact
constants. libyuv cuts one constant short: for BT.709 video at the studio
range (nearly all HD video), blue is taken as 2.0 times the blue
difference where the standard says 2.112 -- libyuv caps the weight at 128
in its 1/64 units unless it is built with `LIBYUV_UNLIMITED_DATA`, which
libavif's copy is not (`gui/video/yuv/src/convert.rs`). So a strongly saturated blue, or its
opposite yellow, comes out up to 14 levels (of 255) from Chrome's; BT.2020's
constant (2.1416) is cut the same way, up to about 18. Everyday colours are
within a level or two.

**Found** 2026-10-10 while holding SDR conversions to Chrome's pixels: the
fixture `gui/video/codec/tests/data/sdr_vp9_bt709_file_pieces.webm` (BT.709,
unconverted) is within two of Chrome's in red and green and up to 14 in
blue, and every off blue is libyuv's with its coefficient capped at 2.0
(`tests/sdr.rs` allows for it, citing this issue).

**Where.** `gui/video/yuv/src/convert.rs` (libyuv's fixed point, the
`kYuvH709Constants` and the rest, transcribed with libyuv's cap) and
`gui/video/yuv/src/reformat.rs` (which takes it where libavif does), as
`gui/video/codec` converts every picture Chrome does not convert by its
colour (design-decisions §1346).

**Why it is so.** §1346 (Claude's) made a frame of video exactly an AVIF
still of the same picture -- libavif's arithmetic, which Chrome's AVIF
decoder uses too -- at lane E's request
(`requests/f-e-vp9-video-decodes-now.md`). The operator's later answer to
F-Q10 (§1378) asks for Chrome's own handling of video, and Chrome's video
is not libavif's: its GPU computes the sum in floating point.

**The proper fix.** Convert ordinary video and AVIF stills as Chrome's GPU
does: Y'CbCr to R'G'B' in floating point with the exact constants, the
chroma brought up as a GPU samples it (bilinear, centred: libavif's slow
path, already `yuv::managed`'s step 1), rounded once -- held to Chrome's
screenshots, as `gui/video/codec/tests/sdr.rs` and
`gui/imagecodec/tests/avif_sdr.rs` hold the converted pictures. A frame of
video and a still of the same picture then still agree (§1346's aim), both
Chrome's. Its cost: the floating-point path is slower than libyuv's
integers; with AVX2 and a frame's bands one to each core it should still
keep up with 4K at 60 frames a second, which needs measuring before the
switch. Lane E is to be told: its request asked for libavif's arithmetic,
and video would part from it.

**What happens until then.** Colours are right to the eye; a saturated
blue or yellow is a few levels, at worst 14-18, from Chrome's.
