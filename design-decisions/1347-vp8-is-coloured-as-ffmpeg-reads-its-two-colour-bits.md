## 1347. VP8 video is coloured as FFmpeg reads its two colour bits: BT.601 at every size, and the clamping bit as the range

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** turning video's YUV into a screen's red, green and blue needs
the recipe it was made with (§1346: the stream's own word first, then the
file's, then a guess from the size -- HD sizes BT.709, smaller BT.601). VP8
has two bits that might be its word. Its key frames carry a "colour space"
bit, whose only defined value means YUV "similar to BT.601", and a "clamping
type" bit, which the specification says is only about whether the decoder
must clamp pixel values. libvpx, VP8's reference decoder, reads both and goes
by neither, so a VP8 file that says nothing else would be guessed by its
size -- an HD VP8 video shown as BT.709. FFmpeg (and every player built on
it) reads the first bit as BT.601 and the second as full range when it is
set. SlateOS reads them as FFmpeg does: VP8 is BT.601 at every size, in the
studio range unless that bit says otherwise.

**What was decided.** `videocodec`'s `ColourHint::vp8`
(`gui/video/codec/src/colour.rs`): colour space bit 0 is matrix 5 (BT.601,
FFmpeg's `BT470BG`), 1 says nothing; clamping type 1 is the full range, 0 the
studio range. As the bitstream's word they stand over the file's `Colour`
element (FFmpeg's decoder overwrites the container's colour the same way); the
primaries, which VP8 never says, are the file's or the guess. The `vp8` crate
hands both bits out with each picture (`Picture::color_space`,
`Picture::clamping_type`), saying what libvpx and FFmpeg each do with them.

**Why FFmpeg's reading, not libvpx's.**
- *What people see.* The fixtures' answers are FFmpeg's (§1346: the colour a
  player would use), and mpv, VLC and ffplay all show VP8 through FFmpeg's
  decoder or its colour tags. Chrome, which decodes VP8 with libvpx, takes
  colour from the container and treats untagged video as BT.601 -- the same
  answer by another road.
- *What VP8 was made with.* libvpx's encoder, which made nearly all VP8 that
  exists, converts from RGB with BT.601's weights at every size. Guessing
  BT.709 for HD VP8 would shift the colours of most of it.
- *Against:* the clamping bit is not a range bit in the specification, and
  reading it as one is FFmpeg's invention. It costs little: libvpx's encoder
  always writes 0 (clamping required), so only a stream from some other
  encoder that sets it is shown as full range -- as FFmpeg shows it.

**Held to.** Five fixtures (`gui/video/codec/tests/data/vp8_*.webm`, among
them a 1280-wide one that untagged VP9 would guess BT.709, and one whose file
says BT.709 that the bitstream overrides), every frame's pixels libavif's
conversion of ffmpeg's decoded planes by the colour ffprobe reports
(`generate_fixtures.py`'s `check_colour`).
