# Lane E -> lane F: `matroska` opens a file cut short inside its Tracks, which FFmpeg refuses

**Filed:** 2026-10-20 by lane E. **For:** lane F (`gui/video/matroska`,
`demux.rs`: `read_description`'s `ids::TRACKS` arm and `read_tracks`).
**Status:** OPEN.

**In short:** take a Matroska file whose download stopped partway through its
track list. FFmpeg won't open it: no streams, no duration. Since 621765524
(2026-10-10), `Demuxer::open` opens it with no tracks and gives the duration
from its Info. `apps/mediaprobe` reports what the demuxer opens, so its test
`a_file_cut_short_says_only_what_its_whole_header_says` has been red since
then. That test is right, and lane E has left it red; it is the only red test
lane E has besides one fixed today.

## What FFmpeg does

The ffprobe build `matroska` is held to (2026-03-09 `9b7439c31b`,
`D:/utils/ffprobe.exe`). The file is an EBML header plus a Segment of known
size holding Info, Tracks (two entries: MPEG-4 video and PCM audio) and one
Cluster: the layout `apps/mediaprobe/src/testing.rs` writes, with no
SeekHead. Each row cuts that file at the point named:

| Cut | ffprobe |
|---|---|
| none (whole file) | video + audio, 1.0 s |
| inside the first TrackEntry | exit 1, `I/O error`, no streams, no duration |
| between the two entries | exit 1, `End of file`, no streams, no duration |
| inside the last entry | exit 1, `I/O error`, no streams, no duration |
| after Tracks, inside the Cluster | video + audio, 1.0 s, "File ended prematurely" |

FFmpeg's own file (SeekHead, Void, Info, Tracks, Tags, Cues, Clusters) gives
the same three refusals when cut at the same places.

## What `matroska` does

mediaprobe's `testing::mkv(1280, 720, 60, 24)` cut 10 bytes before its
Cluster, i.e. inside its last TrackEntry:

    left:  Probe { container: Matroska, duration_secs: Some(60.0), tracks: [], title: None }
    right: Probe { container: Matroska, duration_secs: None,       tracks: [], title: None }

So `open` succeeds and passes over the Tracks as "damaged before its first
entry". It ought to fail with `Error::Truncated`, which mediaprobe maps to the
empty probe.

## Where the line seems to be

In 621765524's case, the phantom Tracks found inside the nine-byte SeekIDs ran
past its *Segment's* stated end. FFmpeg calls that invalid and resynchronises
past it. Here the Tracks lies inside the Segment's stated end, and only the
*file* ends early. FFmpeg reads into it, meets the end of the file, and fails
the header. That points to this rule: a Tracks that runs past the end of the
file but not past its Segment refuses the file however many entries were
read; only one that is damaged within the file, or runs past its Segment,
counts as "damaged before its first entry". Lane E has not traced FFmpeg's
code to confirm this. The table above is the behaviour to match.

## What lane E needs

`Demuxer::open` refusing such a file (`Error::Truncated`). Nothing needs to
change on lane E's side: when the demuxer refuses it, mediaprobe's test goes
green again unchanged.
