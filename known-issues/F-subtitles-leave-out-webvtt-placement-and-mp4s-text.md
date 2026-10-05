### [F] Subtitles leave out WebVTT's placement and MP4's text tracks -- 2026-10-05

**Status:** OPEN (lane F) -- limitations of `videocodec::Subtitles`, each
with what doing it would take.

**In short:** `videocodec::Subtitles` reads a Matroska or WebM film's text
subtitles -- SubRip, ASS, SSA, WebVTT -- into SRT markup for lane E's player
(design-decisions §1360). Three things it does not do yet. A WebVTT cue that
asks to be placed elsewhere -- at the top, or to the left -- comes out at the
bottom centre. An MP4 film's subtitles are not found at all. And reading a
film's subtitles reads the whole film from disk alongside the player.

| What | Today | To do it |
|---|---|---|
| WebVTT's cue settings (`line:0`, `align:start`, `position:10%`): where the cue goes | ignored: every WebVTT cue is bottom centre | read them -- WebM's are the block's second line (`subtitle.rs`, `webm_cue` already finds it), Matroska's `S_TEXT/WEBVTT` the first line of the block's addition (BlockAddID 1, which `Container::next_packet` keeps only for video's alpha today) -- and write `{\anN}`: `line` as a row (an integer from 0 counts from the top, a negative one from the bottom; a percentage by thirds), `align` as a column (`start`/`left`, `center`, `end`/`right`). ffmpeg ignores them, so the answers would be checked against the specification, as WebVTT's other departures are |
| MP4's text tracks: `tx3g` (3GPP timed text, what `ffmpeg -c:s mov_text` and phones write), `wvtt` (WebVTT in MP4), `stpp` (TTML) | `Container::subtitles` returns none for MP4, so such a film has "no subtitles" | `gui/video/mp4` to give subtitle tracks and their samples (it gives video and sound), then a reader each: `tx3g`'s two-byte length, text and `styl`/`hlit` boxes as ffmpeg's `movtextdec` reads them; `wvtt`'s `vttc` boxes as ISO 14496-30 frames them |
| A subtitle reader reads every packet of the file | `Subtitles` opens the film again, as `Sound` does, and the demuxer reads every video and sound packet's bytes to reach the subtitle track's few kilobytes: a second full read of the film while it plays | the Matroska demuxer to skip the payload of blocks whose track is not wanted (seek past the block once its header names the track), set by `Subtitles` and `Sound` for their own track; the walk for key frames still reads each block's header |
| Pictures of text: Blu-ray's PGS, DVD's VobSub, DVB's | refused by name (`Error::SubtitleFormat`) | a decoder each (run-length bitmaps and palettes), and a picture cue type beside the text one |

**Where.** `gui/video/codec/src/subtitle.rs` and `subtitle/`;
`gui/video/codec/src/container.rs` (`subtitles`, `next_packet`);
`gui/video/mp4`; `gui/video/matroska/src/demux.rs`.
