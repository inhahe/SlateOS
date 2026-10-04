### [F] The Matroska demuxer leaves out what no WebM file needs -- 2026-10-04

**Status:** OPEN (lane F) -- limitations, none affecting WebM; each listed
with what doing it would take.

**In short:** `gui/video/matroska` reads WebM files completely, and Matroska
(`.mkv`) files as far as their pictures, sound and subtitles go. A few things
Matroska allows and WebM does not are not read, and a few codecs whose packets
FFmpeg rewrites on the way out come out as stored. None of the test files,
nor any file ffmpeg writes as WebM, uses any of them.

**What is left out, and where it bites.**

| What | Today | To do it |
|---|---|---|
| Tags, chapters, attachments (cover art, fonts for subtitles) | skipped | read `Tags`, `Chapters` and `Attachments` in `demux.rs`'s `read_top_level`, as FFmpeg turns them into metadata, chapters and attached pictures |
| Tracks compressed with bzip2 or LZO | the track is marked unreadable (`Track::readable`) and its packets skipped | undo them in `track.rs`'s `Encoding::undo` (FFmpeg does) |
| Encrypted tracks (WebM's for EME, which ffmpeg passes on still encrypted) | marked unreadable | nothing to decrypt with; a DRM question, not a demuxer one |
| WavPack, ProRes, RealMedia audio and WebVTT packets, which FFmpeg rebuilds from Matroska's form | given as stored | FFmpeg's `matroska_parse_wavpack`, `_prores`, `_rm_audio` and `_webvtt`, if those codecs are ever decoded here |
| Several `ContentEncoding`s on one track | passed through as stored, as FFmpeg does | nothing: FFmpeg's own behaviour |
| A source that cannot seek (a pipe, a live stream) | `Demuxer::open` measures the source, so it needs `Seek` | a streaming mode: the reader already knows where every element ends, but `open`'s SeekHead and the seek's index read ahead |

**Where.** `gui/video/matroska/src/` (`demux.rs`, `track.rs`); the crate's
module documentation says what it reads.

**Was here, fixed 2026-10-04:** a seek in a file without Cues read every
Cluster's block headers to the end of the file, for every seek. It now walks
only to the first key frame past the time and carries on from there next
time (`Walk` in `demux.rs`, design-decisions §1348).
