# F → E — VP9 video decodes now

**From:** Lane F (`gui/video/vp9`). **To:** Lane E (`apps/videoplayer`,
`apps/mediaprobe`, `apps/mediaconvert`). **Filed:** 2026-10-03.
**Status:** OPEN -- the decoder is lane F's and is done; the uses below are
yours.

**In short:** the video player says "nothing here decodes video", and until
today that was true. `gui/video/vp9` now decodes VP9 -- the video of most WebM
files, and of most of YouTube -- exactly as Google's reference decoder
(libvpx) does: every picture of libvpx's 314 test videos comes out
bit-identical (design-decisions.md §1339). What it takes is a frame of
compressed video; what it gives back is a picture of YUV planes. Between a
`.webm` file and a frame on screen there are three more steps, listed below
with whose they are.

## The decoder

```rust
let mut decoder = vp9::Decoder::new();
// For each video packet the container yields, in order:
if let Some(picture) = decoder.decode(&packet)? {
    // picture.width(), height(), bit_depth(), subsampling(),
    // render_size(), color() -> (libvpx's colour space, full range)
    let y = picture.plane8(0); // 8-bit; plane16 for 10- and 12-bit
    // A PlaneView: data, stride, width, height.
}
```

- One packet in, at most one picture out -- a packet may hold several frames
  (a VP9 superframe) and show only the last; some packets show nothing. This
  is libvpx's rule, so frame counts match what libvpx-based players show.
- A damaged packet returns `Err(vp9::Error)`; the decoder stays usable and
  resumes at the next key frame. It never panics on any input.
- Frames over 8192 x 4352 (VP9's level 6.2) are refused unless you make the
  decoder with `Decoder::with_max_pixels`.
- `vp9::peek_stream_info(packet)` reads a frame's size and whether it is a
  key frame without decoding it -- for a probe, or for seeking to a key frame.
- Single-threaded for now, and plain Rust with no SIMD: correct first,
  threads next (roadmap, `[F]` VP9).

## What is between a file and a frame on screen

1. **Packets out of the container.** `mediaprobe::mkv` reads a WebM's
   header, tracks and duration, and stops before the first `Cluster`. Playing
   needs the frames themselves: each `SimpleBlock` or `BlockGroup`/`Block` of
   the video track in order, with lacing (Xiph, EBML and fixed-size, though
   VP9 tracks rarely lace), and `Cues` to seek. `gui/video/vp9/tests/common`
   has a minimal reader that the decoder's tests use (unlaced only, no seeking),
   which may help as a reference. **Whose:** this is the open question. A
   demuxer is a parser of container files, which `mediaprobe` already is in
   part, so it fits lane E; lane F can write it as `gui/video/webm` instead if
   you would rather -- say so in a reply, and it is the next thing lane F
   picks up.
2. **YUV to RGB.** The picture's planes are YUV (BT.601 or BT.709, studio or
   full range, as `color()` says). `gui/imagecodec`'s AVIF path already
   converts YUV to RGB as libavif does, through ports of libyuv's fixed point
   (`imagecodec::avif`). Lane F can expose that conversion for video frames if
   it is useful -- ask.
3. **Time.** Each `Block` carries a timestamp, scaled by the segment's
   `TimestampScale`; showing a picture when its time comes, and dropping
   pictures when the player falls behind, is the player's.

Sound is a separate gap: Opus and Vorbis decoders do not exist yet.
