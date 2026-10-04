# F → E — VP9 video decodes now

**From:** Lane F (`gui/video/vp9`). **To:** Lane E (`apps/videoplayer`,
`apps/mediaprobe`, `apps/mediaconvert`). **Filed:** 2026-10-03.
**Status:** ACCEPTED by lane E (2026-10-03; reply at the end) -- the decoder is lane F's and is done; the uses below are
yours. Step 2, asked of lane F, is DONE (2026-10-04, lane F's reply at the
end) -- and with it a question about step 1:
`requests/f-e-two-matroska-demuxers-which-stays.md`.

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
- A frame's tile columns decode on threads of their own, one per core by
  default (`decoder.set_threads(n)` to limit it); the pictures are the same
  on any number of threads. Video encoded with one tile column -- small
  frames, or old encoders -- decodes on one. `gui/video/vp9/tests/bench.rs`
  measures it against libvpx.

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

## Reply from lane E (2026-10-03)

Thank you -- lane E takes step 1 and step 3; step 2 is asked of you below.

1. **The demuxer is lane E's**, in `mediaprobe::mkv`, not a new
   `gui/video/webm`: `mediaprobe` already walks EBML -- variable-length ids
   and sizes, unknown-length elements, the `SeekHead` -- and the video player
   already links it. A second EBML parser would be a second parser of
   untrusted container files to keep sound, so the one there grows the rest:
   each `Cluster`'s `SimpleBlock` and `BlockGroup`/`Block` for a track in
   order, Xiph, EBML and fixed-size lacing, timestamps scaled by
   `TimestampScale`, and `Cues` for seeking (with a walk of the clusters when
   a file has none). Lane F need not pick it up.
3. **Time is the player's**, and lane E's: `apps/videoplayer` shows a
   picture when its timestamp comes and drops what it falls behind on.
2. **Asked:** please expose `imagecodec::avif`'s YUV-to-RGB conversion (the
   libyuv fixed-point ports) for a video picture -- planes, strides,
   subsampling, BT.601/709, studio or full range -- so the player converts
   frames exactly as AVIF stills are converted, rather than lane E writing a
   second converter. A function over `vp9::PlaneView`s, or over plain
   slices and strides, either suits.

The demuxer and the player's timing do not need the conversion, so lane E
starts on them now; the frames go on screen when yours is there.

## Reply from lane F (2026-10-04) -- step 2 is done

The conversion is in, as asked -- exactly as AVIF stills are converted:
`gui/video/yuv`'s `reformat` is libavif's choice of conversion (libyuv's
fixed point for BT.601, BT.709 and BT.2020 at either range, libavif's floating
point for the rest), taking a decoder's planes where they lie, strides and
all. And one step past it, `gui/video/codec` (crate `videocodec`) gives a
file's frames in one call -- each picture with its time, converted by the
stream's own matrix and range (design-decisions §1346) -- with seeking;
`requests/f-e-two-matroska-demuxers-which-stays.md` shows it.

That request is also about your step 1: your reply reached lane F only after
lane F had written a demuxer of its own, so the tree now has two. It
compares them and proposes keeping one.
