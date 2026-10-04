## 1348. A Matroska seek goes by the file's Cues alone, not by every key frame read since the file was opened

**Date:** 2026-10-04
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** to seek in a video, a player starts decoding at a key frame (a
picture decodable on its own) at or before the time wanted. A Matroska file
usually lists where its key frames are, in an index called the Cues. FFmpeg
also remembers every key frame it has read since it opened the file, and
seeks to the nearest one it knows of -- so where it lands depends on how
much of the file it has already read. The `matroska` crate seeks by the
Cues alone when the file has Cues for the track, and walks the file for key
frames only when it has none. The two land differently only in a file whose
Cues leave key frames out, and then the crate lands on an earlier key frame:
an exact seek still shows the same picture, after decoding a few more
frames to reach it.

**What was decided.** `matroska::Demuxer::seek`
(`gui/video/matroska/src/demux.rs`): the Cues' entries for the track when
there are any; otherwise the key frames found by walking the Clusters from
the first, as far as the first of the track past the time (`Walk`), which is
what FFmpeg's seek does in a file without Cues. The key frames read while
playing are not added to either.

**The alternatives.**

| | Lands on | Depends on what was read before | Reads |
|---|---|---|---|
| The Cues alone (chosen) | the latest *cued* key frame | no | the Cues, once |
| FFmpeg's: the Cues and every key frame read since opening | the latest key frame it has seen | yes | the Cues, once |
| Always walk | the latest key frame | no | the file up to the time, on the first seek there |

- *FFmpeg's* lands nearer after a file has been played through, where its
  Cues are sparse: less to decode for an exact seek. But its answer depends
  on history -- ffprobe, which reads a small file whole while probing it,
  lands elsewhere than a player that seeks as soon as it opens the same
  file -- so no fixture can hold the crate to it without also copying what
  FFmpeg reads while probing; and the index grows with every key frame
  played (FFmpeg halves it when it passes a megabyte).
- *Always walking* reads up to the time sought before the first seek there,
  which the Cues exist to spare -- seconds on a long file over a network.

**Why the Cues alone.** The answer is the same whatever was read before,
which is a fresh FFmpeg's; the Cues are what the muxer wrote for seeking;
and the cost of landing early is decoding time, never a wrong picture.
FFmpeg's and libwebm's muxers cue every video key frame, and so does
mkvmerge by default, so the difference needs a file cued sparsely on
purpose.

**Held to.** `gui/video/matroska/tests/beyond_ffprobe.rs`,
`a_seek_goes_to_a_cue_not_past_it`: `sparse_cues.mkv` has Cues for its first
and third Clusters, and a seek to 1.5 s lands on the first, where ffprobe
lands on the second.

**Revisit if** exact seeks in real files prove slow for this reason: then
feed the key frames read while playing into the walk's list, and seek by
whichever of the two lists is nearer.
