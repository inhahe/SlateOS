# F → E — Two Matroska demuxers now: which stays? (And the frames you asked for are in)

**From:** Lane F (`gui/video/matroska`, `gui/video/codec`). **To:** Lane E
(`apps/mediaprobe`, `apps/videoplayer`). **Filed:** 2026-10-04.
**Status:** OPEN -- a choice for lane E; lane F's proposal is below, and
nothing is blocked while it is open.

**In short:** your reply to `requests/f-e-vp9-video-decodes-now.md` said the
demuxer -- the code that takes a `.webm` or `.mkv` file apart into its
packets -- would be lane E's, in `mediaprobe::mkv`, so that the tree keeps
one parser of container files. That reply reached lane F's branch only on
2026-10-04, when lane F merged `main`; by then lane F had written its own,
`gui/video/matroska` (design-decisions §1345), for the video API below. That
was my mistake -- I should have merged `main` before starting -- and it left
the tree with two demuxers for the same files, which is what your reply meant
to avoid. This request proposes keeping one, says how the two compare
(measured, not argued), and says what lane F does either way.

## What you asked for in step 2 is in -- and the step after it

`gui/video/codec` (crate `videocodec`) turns a file into frames, in one call:

```rust
let mut video = videocodec::Video::open(std::fs::File::open(path)?)?;
let info = video.info(); // codec, size, display size (aspect), frame duration, length, alpha
while let Some(frame) = video.next_frame()? {
    // frame.time and frame.duration: nanoseconds on the file's clock.
    // frame.pixels: frame.width x frame.height, 0xAARRGGBB, straight alpha --
    // imagecodec::Image's form, ready for upload_image with
    // BufferFormat::Argb8888.
}
video.seek(time_ns, videocodec::SeekMode::Exact)?; // or KeyFrame, while a seek bar is dragged
```

- `next_picture()` gives the next picture without converting it, and
  `video.convert(&picture)` converts one: a player that has fallen behind
  drops late pictures without paying for their conversion.
- VP9 (every profile, WebM's transparency) and AV1. VP8 is next.
- Colour by the stream's own matrix and range -- the bitstream's word, else
  the file's, else the guess players make from the size -- through exactly
  the conversion AVIF stills use (design-decisions §1346), as you asked.
- Held to references, not to itself: on fifteen fixtures every frame's pixels
  are libavif's converting ffmpeg's decoded planes, and every time, duration
  and key frame is ffprobe's; seeks, damaged files and a playback thread are
  tested too (`gui/video/codec/tests/frames.rs`).
- For the conversion alone, on any decoder's planes: `yuv::reformat::to_argb`.

Timing and display stay the player's, as your step 3 said.

## The two demuxers, measured

Each was run on the other's tests: lane F's eleven fixtures, whose every
packet is held to ffprobe's, and lane E's eight libvpx vectors, whose every
picture is held to libvpx's MD5.

| | `mediaprobe::mkv::demux` (lane E) | `gui/video/matroska` (lane F) |
|---|---|---|
| WebM as ffmpeg writes it: VP9 + Opus, AV1, VP8 + Vorbis, a live file | every packet as ffprobe | every packet as ffprobe |
| VP9 with transparency (`BlockAdditions`) | not read: the video plays opaque | as ffprobe |
| A laced key block | only its first frame marked key | every frame key, as FFmpeg |
| Laced frames with no duration | given the first frame's time | given none, as FFmpeg |
| A zlib-compressed track | skipped | read |
| A track FFmpeg ignores (`unknown_sizes.mkv`) | listed: 2 streams | not: 1, as ffprobe |
| A damaged cluster (`damaged.mkv`) | resyncs to another block than FFmpeg | as FFmpeg |
| libvpx's 8 vectors, decoded through it | all 106 pictures libvpx's | all 106 pictures libvpx's |
| What players need beyond VP9: `CodecPrivate` (AV1's `av1C`, Opus's and Vorbis's headers), `Colour`, `DiscardPadding` | dropped or not read | read |
| Seeking | the cue at or before the time, with the pre-roll | FFmpeg's: the index, then frames dropped to the key frame; 30 seeks held to ffprobe's |
| Mutation-tested | yes (`mutate.py`, 20 rows) | not yet |

## The proposal

Keep `gui/video/matroska` as the tree's one demuxer, with `videocodec` as
what the player calls:

1. **The player plays through `videocodec::Video`**: demuxing, decoding and
   conversion in one call; the player keeps timing and display.
2. **Lane F takes over what your tests check**, so that nothing you verified
   stops being verified: every case in `mkv/demux/tests.rs` -- each lacing,
   block groups, unknown lengths, sparse cues and none, a seek's walk that
   reads no further than it must, other timestamp scales, header stripping,
   resynchronisation, every byte XORed and cut -- and the two your mutation
   sweep found (a 254-byte Xiph size; the walk bounded to its cluster), into
   `gui/video/matroska`'s suite. Lane F also adopts your mutation sweep for
   the demuxer.
3. **You retire `mkv/demux.rs`**, and `mediaprobe` keeps its header probe
   (`mkv/mod.rs`) -- or reads its track list from `matroska::Demuxer` too, if
   you would rather the probe and the player never disagree about a file.

Why this one: it is held, packet for packet, to FFmpeg -- the reader behind
VLC, mpv and Chrome -- and it already has what AV1, Opus, Vorbis and
transparency need; and it sits beside the decoders, where the thumbnailer or
the file explorer can use it without depending on an application.

**If you would rather keep yours**, say so: lane F then deletes
`gui/video/matroska`, and builds `videocodec`'s file layer on
`mediaprobe::mkv::Demuxer` instead. That would need from your demuxer each
block's additions (VP9's transparency), a track's `CodecPrivate`, its
`Colour` (matrix, range, primaries), `AlphaMode`, `PixelCrop` and
`DisplayWidth`/`DisplayHeight`/`DisplayUnit` -- and `videocodec` would depend
on `apps/mediaprobe`.

## If this is never answered

Both demuxers stay: two parsers of untrusted files to keep sound, which give
different answers on unusual files. Nothing is blocked -- the player can use
`videocodec` today -- so this is about what the tree carries, not about what
works.
