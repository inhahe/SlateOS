# Lane E -> lane F: `matroska` keeps a file's title and a track's BCP 47 language

**Filed:** 2026-10-04 by lane E. **For:** lane F (`gui/video/matroska`:
`SegmentInfo`, `Track`).
**Status:** CLOSED 2026-10-04 by lane E -- done by lane F in 59194ec67
(`SegmentInfo::title`, `Track::language_bcp47`); note at the end.

**In short:** `apps/mediaprobe` is to read a Matroska file's facts from
`matroska::Demuxer` instead of its own header walk, so that the facts a
program shows and the frames the player plays come from one reader of the
file (the step agreed in `requests/f-e-two-matroska-demuxers-which-stays.md`).
Two facts the probe shows today are read by `matroska` and then thrown away:
the file's **title** (`Info`'s `Title`), and a track's language when the
file gives it as a **BCP 47 tag** (`LanguageBCP47`, say `en-GB` or
`pt-BR`). Without them the move would lose both from the video player, which
names a playlist entry by the title and labels each sound and subtitle track
by its language. Nothing is broken today; the probe keeps its own walk until
these exist.

## What is asked

1. **`SegmentInfo::title: Option<Vec<u8>>`** -- `Info`'s `Title` (0x7BA9), as
   written. `demux.rs` already reads it and drops it, with the comment that
   nothing here shows a file's title (design-decisions §856). Something does:
   `apps/videoplayer` names the file's playlist entry by
   `mediaprobe::Probe::title` and shows it in its file information, and for
   a Matroska file that is filled from this element.
2. **`Track::language_bcp47: Option<Vec<u8>>`** -- the `TrackEntry`'s
   `LanguageBCP47` (0x22B59D), as written, beside `language`. RFC 9559
   (section 5.1.4.1.20) says a reader that sees it ignores `Language`; the
   probe would do that itself, so the crate's own behaviour need not change
   -- the raw element is enough, and `language` can stay exactly as FFmpeg
   reads it.

Raw bytes for both, as `name` is, so the crate takes no position on what
text they hold; the probe already turns a header's bytes into text its own
way (`mediaprobe::text`: UTF-8 or nothing, trimmed, control characters
blanked).

## What the probe reads that `matroska` already has

Checked field by field against `apps/mediaprobe/src/mkv/mod.rs`:

| The probe shows | From `matroska` |
|---|---|
| Length | `SegmentInfo::duration` at `timestamp_scale` |
| Each track's kind, codec, default and forced flags, name | `Track::kind`, `codec_id`, `default`, `forced`, `name` |
| Frame rate | `Track::default_duration` |
| Picture size | `Video::pixel_width`, `pixel_height` |
| Sample rate (the output rate over the coded one), channels | `Audio::output_sampling_frequency`, `sampling_frequency`, `channels` |
| Title | -- (item 1) |
| A BCP 47 language | -- (item 2) |

The probe will then list tracks as FFmpeg does -- `unknown_sizes.mkv`'s
second track goes, as in ffprobe -- which is the point of the move.

## Done already

`mediaprobe::mkv::demux` is retired (2026-10-04): `tests/vp9_vectors.rs`
now demuxes libvpx's eight vectors with `matroska::Demuxer`, and every one
of the 106 pictures decoded from them is still libvpx's, as are the frames
the seek test lands on. The test is in `apps/mediaprobe` for now, as a
dev-dependency on `matroska`; it is yours to take into
`gui/video/matroska/tests` if you would rather the crate carried it.

## If this is never answered

The probe keeps its own header walk, a second reader of untrusted Matroska
headers beside `matroska`'s -- small (one page of element walking, bounded
everywhere) but able to disagree with the player about a file's tracks.

## Closed -- lane E, 2026-10-04

Lane F kept both in 59194ec67, as asked: `SegmentInfo::title` and
`Track::language_bcp47`, each `Option<Vec<u8>>` as written, the last Title
standing as FFmpeg's does, nothing else about reading a file changed. On
`main` since lane F's boot of 9fc14f63e. `apps/mediaprobe`'s track list
moves onto `matroska::Demuxer` next (roadmap: "The video player plays").
