### [F] The Matroska demuxer leaves out what no WebM file needs -- 2026-10-04

**Status:** OPEN (lane F) -- limitations, none affecting WebM; each listed
with what doing it would take.

**In short:** `gui/video/matroska` reads WebM files completely, and Matroska
(`.mkv`) files as far as their pictures, sound, subtitles, chapters, tags and
attachments go. A few things Matroska allows and WebM does not are not read,
and a few codecs whose packets FFmpeg rewrites on the way out come out as
stored. None of the test files, nor any file ffmpeg writes as WebM, uses any
of them.

**What is left out, and where it bites.**

| What | Today | To do it |
|---|---|---|
| Tracks compressed with bzip2 or LZO | the track is marked unreadable (`Track::readable`) and its packets skipped | undo them in `track.rs`'s `Encoding::undo` (FFmpeg does) |
| Encrypted tracks (WebM's for EME, which ffmpeg passes on still encrypted) | marked unreadable; the key ID is in the track's metadata (`enc_key_id`) | nothing to decrypt with; a DRM question, not a demuxer one |
| WavPack, ProRes, RealMedia audio and WebVTT packets, which FFmpeg rebuilds from Matroska's form | given as stored | FFmpeg's `matroska_parse_wavpack`, `_prores`, `_rm_audio` and `_webvtt`, if those codecs are ever decoded here |
| Several `ContentEncoding`s on one track | passed through as stored, as FFmpeg does | nothing: FFmpeg's own behaviour |
| A source that cannot seek (a pipe, a live stream) | `Demuxer::open` measures the source, so it needs `Seek` | a streaming mode: the reader already knows where every element ends, but `open`'s SeekHead and the seek's index read ahead |
| A top-level element of unknown size other than a Cluster (the specification allows it of none) | refused before the first Cluster; ends following the SeekHead there | FFmpeg reads one until an element that cannot be inside it begins, as it reads a Cluster of unknown size |
| A chapter without an end, in a file whose Info gives no duration | the last such chapter ends where it starts (`Demuxer::chapter_ends`) | FFmpeg estimates a duration from the streams' bit rates when probing; that needs the bit rates, which the demuxer does not know |
| Damage in an Info or Tracks before the first Cluster | the file is refused | FFmpeg reads the Segment again from its start, which repeats every track before the damage (design-decisions §1358 for why that is not copied for chapters, tags and attachments); reading on after the damaged element, as there, would serve |

**Where.** `gui/video/matroska/src/` (`demux.rs`, `track.rs`, `nest.rs`,
`metadata.rs`); the crate's module documentation says what it reads.

**Was here, fixed 2026-10-05:** with several Cues elements before the first
Cluster, the first alone was the index, where FFmpeg seeks by all of them
(`two_cues.mkv`); and damaged Cues were no index at all, where FFmpeg seeks
by the points before the damage (`damaged_cues.mkv`). Both now as FFmpeg,
held to ffprobe probing as little as it can -- its probing of a small file
indexes every key frame, and hides what the Cues say.

**Was here, fixed 2026-10-05:** chapters, tags and attachments were skipped.
They are read now, and the metadata FFmpeg makes of them and of the Info
and the tracks is given as ffprobe shows it (`Demuxer::metadata`,
`chapters`, `attachments`, `Track::metadata`; design-decisions §1358).

**Was here, fixed 2026-10-04:** a seek in a file without Cues read every
Cluster's block headers to the end of the file, for every seek. It now walks
only to the first key frame past the time and carries on from there next
time (`Walk` in `demux.rs`, design-decisions §1348).
