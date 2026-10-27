### [F] An MP4's AAC sound and HEVC video are not decoded yet, and a few MP4 corners are left out -- 2026-10-04

**Status:** OPEN (lane F) -- limitations; the first is the one a user meets.

**In short:** the video player's library (`gui/video/codec`, `videocodec`)
opens MP4 files as well as WebM and Matroska, and plays their VP8, VP9, AV1
and -- since 2026-10-10 -- H.264 video exactly as ffmpeg does, edit lists,
colour, pixel shape, rotation and crop included. What a user still meets:
an MP4's sound is nearly always AAC, which is not decoded yet, so most MP4
films show their pictures without their sound; and HEVC video is refused
by name. A few rarer things are also left out, listed below with what doing
each would take.

**What is left out, and where it bites.**

| What | Today | To do it |
|---|---|---|
| HEVC video -- what newer phones record | refused by name (`Error::Codec(Codec::Hevc)`) | a decoder, an optional install (design-decisions §1370). H.264 plays since 2026-10-10: FFmpeg's decoder translated, `gui/video/h264` (§1376) |
| Interlaced H.264 -- TV recordings, older camcorders | shown as FFmpeg gives it: each picture's two fields woven together, which combs where things move | a deinterlacer (FFmpeg's players use `bwdif` or `yadif`), between the decoder and the conversion |
| Matroska H.264 whose parameter sets do not say how far it reorders | the decoder starts holding back what they say (or nothing) and holds back more once it sees it must, losing the first pictures it can then no longer show in order | probe the stream first, as FFmpeg's player does (`avformat_find_stream_info`); `gui/video/h264`'s tests already copy that probe (`probe_delay`). An MP4 needs none: its reorder is estimated from its composition offsets, as FFmpeg's demuxer estimates it -- a fragmented one's from its fragments', once they are read, where FFmpeg's demuxer has none and its player probes. Either way the decoder then raises it to what the parameter sets say, as x264's do (`h264_fragmented.mp4`) |
| A display matrix that turns by other than a quarter turn | the picture is shown as it is, as mpv shows it | ffmpeg turns it with its `rotate` filter and fills the corners black; nothing asks for that here (`gui/video/codec/src/orientation.rs`) |
| Spherical (360-degree) video: MP4's `sv3d`, Matroska's equirectangular and cubemap projections | shown as its flat, unwrapped picture | a viewer that maps it onto a sphere; nothing here has one. (Matroska files FFmpeg refuses for a broken projection are refused, as there.) |
| A fragmented MP4's `sidx` (segment index) | not read: every fragment's `moof` is read when the file opens, so a long fragmented file costs a read of each fragment's header before the first frame | read `sidx` as FFmpeg does (`mov_read_sidx`) and the fragments as they are reached; the packets come out the same either way, only the time to open changes (`gui/video/mp4/src/parse.rs`) |
| Encrypted MP4 tracks (`encv`, `enca`: Common Encryption) | their codec is not recognised, so they are not played | nothing to decrypt with; a DRM question, not a demuxer one |
| Sound in AAC -- what most MP4 files carry | refused by name (`Error::SoundCodec(SoundCodec::Aac)`); Opus in MP4 plays (2026-10-04) | an AAC decoder, beside `gui/video/opus` and `gui/video/vorbis`: FFmpeg's fixed-point decoder translated, included (decided 2026-10-09, design-decisions §1377); the roadmap's "Video files" item |

**Where.** `gui/video/codec` (`container.rs`, `video.rs`, `orientation.rs`),
`gui/video/mp4`. The crates' module documentation says what they read.
