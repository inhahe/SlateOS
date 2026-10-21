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
| WavPack, ProRes and RealMedia audio packets, which FFmpeg rebuilds from Matroska's form | given as stored | FFmpeg's `matroska_parse_wavpack`, `_prores` and `_rm_audio`, if those codecs are ever decoded here. (WebVTT's, which FFmpeg's `matroska_parse_webvtt` takes apart, are given as stored too, and `videocodec::Subtitles` takes WebM's apart as it does.) |
| Several `ContentEncoding`s on one track | passed through as stored, as FFmpeg does | nothing: FFmpeg's own behaviour |
| A source that cannot seek (a pipe, a live stream) | `Demuxer::open` measures the source, so it needs `Seek` | a streaming mode: the reader already knows where every element ends, but `open`'s SeekHead and the seek's index read ahead |
| A master element of unknown size inside another of unknown size -- a `Tag` in a `Tags`, a `TrackEntry` in a `Tracks` (the specification allows it of none; a top-level element of unknown size is read, as FFmpeg reads one) | damage: the element holding it keeps what was read before it | FFmpeg reads one until an element its parent or any level above knows begins, handing that element up (`ebml_parse`'s walk up its syntax tables). It needs each reader in `nest.rs` and `track.rs` to know the IDs of the levels above it. ffprobe 9b7439c31b lists the tag of a `Tag` of unknown size in a `Tags` of unknown size (probed 2026-10-21) |
| A chapter without an end, in a file whose Info gives no duration | the last such chapter ends where it starts (`Demuxer::chapter_ends`) | FFmpeg estimates a duration from the streams' bit rates when probing; that needs the bit rates, which the demuxer does not know |
| Damage in a Tracks after its first track (damage before its first is passed over, as FFmpeg passes over it) | before the first Cluster, the file is refused; in a Tracks the SeekHead names, after the Clusters, its tracks are left out and following the SeekHead stops | FFmpeg keeps the tracks read before the damage, and the damaged entry's fields read before it, wherever the Tracks is. Before the first Cluster its second pass then reads a Tracks the first pass read again, which lists those tracks twice (design-decisions §1358 for why that is not copied) -- or, where it meets a Cluster's ID first (a SeekHead naming a Cluster, before the Tracks), lists them once and opens the file; and a Tracks only the second pass reads (after a stray byte, say) it reads once, and opens the file. Keeping the tracks read before the damage, once, and reading on -- as for damaged Tags -- would serve every case, with each track's fields read as FFmpeg's partial `TrackEntry` holds them |
| Where playing resyncs from, the first Cluster being damaged, in a file whose description FFmpeg read in one pass and whose SeekHead it followed nowhere | one byte past the start of the last top-level element before the Cluster: reading passes over the description again from the Segment's start, to reach it | one byte past the start of the last element FFmpeg knew inside that one -- the last `TrackEntry`'s last child, say: the two differ only where a top-level element's ID lies in the bytes between. Doing it needs every reader in `nest.rs` and `track.rs` to say where the last element it knew began, and to know which IDs FFmpeg knows at its level. (After a second pass, or a SeekHead entry followed, FFmpeg resyncs from the first Cluster, and so does this: `resync_first_cluster_after_seek_head.mkv`) |
| More than 1000 tracks | read, every one | FFmpeg refuses the file: `matroska_parse_tracks` makes a stream of each track with a codec, and a file's streams stop at its `max_streams` of 1000. A track takes many times the 13 bytes a `TrackEntry` can be written in, so the limit would also bound what a file of nothing but them makes the demuxer hold |
| Elements between the EBML header and the Segment (a Void, say) | passed over, and the Segment read from its start | FFmpeg reads one of them where it expects the Segment, which sends it straight to its second pass: the Segment's elements are found by their IDs, and its `segment_start` never set, so it counts the SeekHead's and the Cues' positions from the file's start and misses what they point to. Copying it would copy a fault |
| The end of the file in a `TrackEntry`, between two of its elements | the file is refused, as the end of the file inside an element is | FFmpeg reads that as damage its second pass looks past, not as the file cut short: the two differ only where an element's ID lies in the entry's bytes after the damage. Reading entries with `nest::child`, as the other readers are, would do it |

**Where.** `gui/video/matroska/src/` (`demux.rs`, `track.rs`, `nest.rs`,
`metadata.rs`); the crate's module documentation says what it reads.

**Was here, fixed 2026-10-21:** damage before the first Cluster was looked
past by rules of this crate's own -- one for stray bytes, one for an
element running past the Segment's stated end, one for the end of the file
-- each matching FFmpeg on its own fixture and not on the cases around it: a
stray byte in a Segment of unknown size, or two bytes of `FF` fill, refused
a file FFmpeg opens; an Info running past the Segment's stated end was
passed over unread where FFmpeg reads it; a file cut short inside a
SeekHead opened where FFmpeg refuses it; and a top-level element of unknown
size refused the file. It is now FFmpeg's second pass itself
(`Demuxer::second_pass`), and Cues met before the first Cluster are read as
the file is opened, so that damage in them starts that pass as it does in
FFmpeg. Held to ffprobe by 37 files (`openings.txt`). And a Tracks after the
Clusters with an entry FFmpeg refuses the file for -- a crop leaving nothing
-- left its tracks out where FFmpeg refuses the file; it refuses it now
(`meta_refused_track_after_the_clusters.mkv`).

**Was here, fixed 2026-10-05:** a damaged Info before the first Cluster
refused the file. It keeps the fields read before the damage and reading
goes on, as FFmpeg plays it (`meta_damaged_info.mkv`).

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
