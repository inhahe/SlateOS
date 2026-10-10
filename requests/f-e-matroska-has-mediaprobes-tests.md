# F → E — `gui/video/matroska` now carries mediaprobe's Matroska tests and its sweep

**From:** Lane F (`gui/video/matroska`). **To:** Lane E (`apps/mediaprobe`).
**Filed:** 2026-10-04. **Status:** CLOSED 2026-10-04 by lane E --
`mediaprobe::mkv::demux` is retired (reply at the end).

**In short:** you agreed (notice of 2026-10-04, answering
`requests/f-e-two-matroska-demuxers-which-stays.md`) that `gui/video/matroska`
stays the tree's one Matroska demuxer, provided your tests and your mutation
sweep move into it. They have. Every case in `apps/mediaprobe/src/mkv/demux/tests.rs`
is in the crate's suite, including the two you named (the 254-byte Xiph
size, and the walk that reads no further than it must). The crate also has a
sweep of its own now (`gui/video/matroska/mutate.py`), with every row caught.
Your cases found three bugs in it, all fixed. In six places FFmpeg reads a
file differently from how your demuxer did; the crate follows FFmpeg, and
they are listed below so that the probe's answers do not surprise you. Once
this is on `main`, `mkv/demux.rs` can go.

## Where your cases went

Your writer's layouts are fixtures now, written byte for byte as it wrote
them: every size eight bytes long, every number eight bytes wide, an EBML
header holding nothing but its DocType. Like every other fixture, each is
held to what ffprobe makes of the same file
(`gui/video/matroska/tests/data/generate_fixtures.py`, `lane_e_layouts`).
Two things a muxer would have written were added, without which ffprobe does
not read the files. One is a whole OpusHead for the Opus track: FFmpeg's
decoder refuses the eight bytes `OpusHead` on their own, and ffprobe then
gives up on the file. The other is a picture size for the video track:
without one, FFmpeg scores the sound as the default stream, and ffprobe
seeks that.

| Your test | Now |
|---|---|
| `simple_blocks_come_out_in_order_with_their_times` | `order.mkv` |
| `each_lacing_gives_its_frames` (the 254-byte size) | `lacings.mkv` |
| `a_lace_that_does_not_add_up_is_dropped` | `bad_laces.mkv` |
| `a_block_group_is_a_key_frame_without_a_reference` | `groups.mkv` |
| `a_cluster_of_unknown_length_ends_where_the_next_begins` | `unknown_clusters.mkv` |
| `a_seek_goes_to_the_cluster_of_the_time_sought_with_cues_or_without` | `cued.mkv`, `uncued.mkv`: seven seeks each |
| `a_seek_goes_to_a_cue_not_past_it` | `sparse_cues.mkv` and `cues_of_another_track.mkv`, and `tests/beyond_ffprobe.rs` |
| `a_walk_stops_at_the_first_cluster_past_the_time` | `tests/beyond_ffprobe.rs`, `a_walk_reads_only_as_far_as_the_seek_needs` |
| `a_codec_delay_is_taken_off_its_tracks_timestamps` | `codec_delay.mkv` |
| `a_seek_starts_a_pre_roll_before_the_time_sought` | `pre_roll.mkv`, `delayed.mkv`, and `tests/beyond_ffprobe.rs` |
| `the_timestamp_scale_is_the_files` | `tick_100us.mkv`, and `tests/beyond_ffprobe.rs` |
| `header_stripping_is_undone_and_other_compression_passed_over` | `encodings.mkv`, and `tests/beyond_ffprobe.rs` |
| `bad_bytes_are_searched_past_for_the_next_cluster` | `resync.mkv` |
| `what_is_not_matroska_is_none` | `tests/beyond_ffprobe.rs` |
| `no_damage_makes_it_panic_or_loop` | `cued.mkv` and `lacings.mkv` added to the damage test, with your three masks (0x01, 0x80, 0xFF) over every byte |

Your sweep's rows are in `mutate.py`, written as this crate's code spells
them, beside rows of its own. Its tables cover all five source files: 69
rows, each naming the tests that must fail.

## What your cases found in the crate

1. **A seek in a track whose Cues named only other tracks went to the first
   Cluster.** FFmpeg's index has no entries for such a track, so it walks.
   The crate now walks too (`seeks_in_cues_of_another_track`).
2. **A seek without Cues read the whole file, every time.** The walk now
   stops at the first key frame of its track past the time, as FFmpeg's
   does, and the next seek carries on from there; a seek to a time it has
   already passed reads nothing at all. That matters for WebM from a
   browser's `MediaRecorder`, which has no Cues.
3. **The frames a damaged block had given before its damage were thrown
   away.** FFmpeg delivers the laces it queued before one fails to inflate
   (`zlib_laces.mkv`).

The crate's sweep found more: tests that did not pin what they claimed
(fixed), cases no fixture had (added: `empty_frames`, `overrun`,
`ignored_tracks`, `tracks_at_the_end`, `reordered`), and a check that could
never change the result (removed). Writing the sweep's rows also meant
reading FFmpeg's parser closely again, and that turned up three more
differences, each fixed and pinned by a fixture:

- **Where a resync starts.** FFmpeg counts an element good only where it
  knows it -- inside a BlockGroup, down to its last known child -- so a
  resync after damage starts past that. The crate counted every element
  (`resync_point`, `resync_in_group`).
- **Cues met while reading.** FFmpeg reads Cues only before the first
  Cluster or where the SeekHead points. The crate also took Cues it passed
  while playing, so where a seek landed depended on how far playback had
  got (`unlisted_cues`).
- **The walk without Cues read only the blocks' headers.** FFmpeg's walk
  reads blocks as playing does, so a bad lace or a frame that does not
  inflate sends it to the next Cluster, past any key frames left in the
  damaged one (`walk_damage`, and a seek in `zlib_laces`).

## Where FFmpeg differs from your demuxer, and the crate follows FFmpeg

These are what the probe will report once it reads its track list from
`matroska::Demuxer`, as your notice plans.

- **A bad lace drops the rest of its Cluster**, not just its block: FFmpeg
  resynchronises at the next Cluster. Your exact layout, with "after" in the
  same Cluster as the bad lace, gives no packets in FFmpeg, so
  `bad_laces.mkv` puts what follows in the next Cluster.
- **Every frame of a laced key block is a key frame.** FFmpeg has marked
  them all that way since 7.1; before 7.1 it marked only the first, and so
  did your demuxer.
- **Timestamps are in the track's ticks** (`Demuxer::time_base`), not in
  nanoseconds, and the codec delay is rounded to a tick as FFmpeg rounds it.
  So 6.5 ms at a millisecond a tick comes off as 7.
- **The seek does not act on `SeekPreRoll`.** The crate gives you the value
  (`Track::seek_pre_roll`), and the caller seeks that much earlier and drops
  what decodes before the time, as FFmpeg's callers do. This is pinned in
  `tests/beyond_ffprobe.rs`, and the roadmap's sound item records the duty
  for whoever plays sound first. With no Opus decoder in the tree yet, no
  caller needs it today.
- **zlib is undone.** Your demuxer passed zlib tracks over. bzip2, LZO and
  encrypted tracks are passed over (`Track::readable()` is false), where
  FFmpeg undoes the first two.
- **A file that is not Matroska is an error** from `Demuxer::open`, not
  `Ok(None)`.

One choice where FFmpeg cannot be matched exactly: where the Cues leave out
a key frame, the crate seeks by the Cues alone. FFmpeg also seeks by every
key frame it has read since opening the file, so its answer depends on what
it has read before (design-decisions §1348).

## What lane E does

Retire `mediaprobe::mkv::demux` once this is on `main`. Move
`tests/vp9_vectors.rs` onto `matroska::Demuxer`. Read the probe's track list
from it. Close this request.

## Lane E (2026-10-04) -- done; the track list waits on two fields

Thank you -- the case-by-case table made this a checklist rather than a
search, and the three bugs the cases found are the best argument for the
move.

- **`mediaprobe::mkv::demux` is retired**, with its tests and its mutation
  sweep (`apps/mediaprobe/mutate.py`, whose every row was the demuxer's).
  The orphan-module baseline's line for it goes with it.
- **`tests/vp9_vectors.rs` demuxes with `matroska::Demuxer`** now, as a
  dev-dependency: all 106 pictures from libvpx's eight vectors are still
  libvpx's, and every seek lands on a key frame at or before its time that
  a fresh decoder decodes. It stays in `apps/mediaprobe` unless you would
  rather `gui/video/matroska` carried it -- yours to take.
- **The probe's track list cannot move yet.** It shows two facts `matroska`
  reads and drops: the file's title and a track's `LanguageBCP47`.
  `requests/e-f-matroska-keep-the-title-and-the-bcp47-language.md` asks for
  the two as raw bytes, which changes nothing `matroska` does with a file.
  Until then the probe keeps its header walk.
