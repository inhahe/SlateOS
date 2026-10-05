## 1361. A reader of one track passes over the others' blocks unread, and reads ahead to suit

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** a player opens a film three times -- once each for its
pictures, its sound and its subtitles (`videocodec::Video`, `Sound`,
`Subtitles`) -- and each used to read the whole film from the disk, keeping
only its own part. Now each asks the Matroska reader for its own track
alone, and the reader skips the other tracks' data without reading it. On a
minute of 1080p film, the subtitles now read about 4% of the file, not all
of it. The sound does the same; the pictures read what they did.

**What was decided.**

1. **`matroska::Demuxer::select_tracks`**: another track's block is passed
   over once its header has named its track, its bytes never read. FFmpeg
   does the same for a stream it discards (`AVDISCARD_ALL`): it returns
   from `matroska_parse_block` right after reading the track number. As
   there, those blocks then count for nothing: not in a seek's dropping of
   what comes before its time, nor among the key frames a seek without
   Cues walks to. Read alone, every track of every fixture the demuxer
   opens gives exactly the packets it gives among the others, and every
   fixture read seven bytes ahead at a time reads as at 64 KiB
   (`beyond_ffprobe.rs`).
2. **How far ahead the reader reads is the caller's**
   (`Demuxer::set_read_ahead`, 64 KiB at first). A passed-over block costs
   a read of that much past it, so a reader that skips most of the file
   wants little; one that reads it through wants much (fewer reads). One
   minute of 1080p film at 5 Mbit/s with sound and subtitles (39 MB),
   measured (`film_read_track_by_track`):

   | Read ahead | Subtitles alone: bytes | reads | All tracks: reads |
   |---|---|---|---|
   | 64 KiB | 82% | 487 | 594 |
   | 4 KiB | 15% | 1442 | 2882 |
   | 2 KiB | 7.7% | 1442 | 2882 |
   | 1 KiB | 4.3% | 1563 | 3003 |
   | 512 B | 4.0% | 2883 | 4444 |

   `Sound` and `Subtitles` read ahead 1 KiB (`PASSING_READ_AHEAD`): about a
   read a video frame, and less of the file than 2 KiB without the doubled
   reads of 512 B. `Video` keeps 64 KiB: the pictures are most of the bytes,
   and read through, the large buffer takes a fifth of the reads.

**Alternatives considered.**

- **One small read-ahead for everyone** (2-4 KiB). *For:* simpler, and on a
  local disk no slower to read a whole file (large frames go straight into
  their own memory either way). *Against:* about five times the reads for
  the pictures. Each is a round trip to the file server on SlateOS, and
  across a network more.
- **Demuxing once and handing each reader its packets.** *For:* the file is
  read once, all told. *Against:* it couples the three readers, which a
  player runs on its own threads at its own pace (the sound ahead of the
  pictures, the subtitles anywhere), and `videocodec`'s API is one reader a
  stream. With the passing-over, the second and third reads cost a few
  percent of the first.

**MP4 the same** (`mp4::Demuxer::select_tracks`, `set_read_ahead`): FFmpeg's
`mov_read_packet` reads nothing of a discarded stream's samples, which
still take their turns in the order of the packets, so a selected track's
come out as among the others (every track of every MP4 fixture, held to
it). The same minute of film as MP4 (AAC sound): the sound alone reads
81.8% of the file 64 KiB ahead, 15.2% 4 KiB ahead, 3.8% 1 KiB ahead, at a
read a video frame -- so the same 1 KiB. Ogg's pages carry their streams
together, and are read whole.
