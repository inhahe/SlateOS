### [F] MP4 plays only in the codecs WebM has, and a few MP4 corners are left out -- 2026-10-04

**Status:** OPEN (lane F) -- limitations; the first is the one a user meets.

**In short:** the video player's library (`gui/video/codec`, `videocodec`)
now opens MP4 files as well as WebM and Matroska, and plays their VP8, VP9
and AV1 video exactly as ffmpeg does -- edit lists, colour, pixel shape,
rotation and crop included. But most MP4 files in the world hold H.264 (or
HEVC) video, which nothing in SlateOS decodes yet: such a file is refused
with "the video is H.264, which is not decoded here yet". A few rarer MP4
features are also left out, listed below with what doing each would take.

**What is left out, and where it bites.**

| What | Today | To do it |
|---|---|---|
| H.264 and HEVC video -- what phones, cameras and most of the web write into MP4 | refused by name (`Error::Codec(Codec::H264)`, `Hevc`) | a decoder for each, beside `gui/video/vp9` and `rav1d`; the roadmap's "Video files" item. Whether to include one, and which code to start from, waits on the operator: `open-questions/F-Q8.md` (H.264) and F-Q1 (HEVC) |
| A display matrix that turns by other than a quarter turn | the picture is shown as it is, as mpv shows it | ffmpeg turns it with its `rotate` filter and fills the corners black; nothing asks for that here (`gui/video/codec/src/orientation.rs`) |
| Spherical (360-degree) video: MP4's `sv3d`, Matroska's equirectangular and cubemap projections | shown as its flat, unwrapped picture | a viewer that maps it onto a sphere; nothing here has one. (Matroska files FFmpeg refuses for a broken projection are refused, as there.) |
| A fragmented MP4's `sidx` (segment index) | not read: every fragment's `moof` is read when the file opens, so a long fragmented file costs a read of each fragment's header before the first frame | read `sidx` as FFmpeg does (`mov_read_sidx`) and the fragments as they are reached; the packets come out the same either way, only the time to open changes (`gui/video/mp4/src/parse.rs`) |
| Encrypted MP4 tracks (`encv`, `enca`: Common Encryption) | their codec is not recognised, so they are not played | nothing to decrypt with; a DRM question, not a demuxer one |
| Sound | no track's sound is played yet | Opus, AAC and Vorbis decoders; the roadmap's "Video files" item |

**Where.** `gui/video/codec` (`container.rs`, `video.rs`, `orientation.rs`),
`gui/video/mp4`. The crates' module documentation says what they read.
