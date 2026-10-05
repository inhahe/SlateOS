### [F] Subtitles leave out MP4's text tracks and pictures of text -- 2026-10-05

**Status:** OPEN (lane F) -- limitations of `videocodec::Subtitles`, each
with what doing it would take.

**In short:** `videocodec::Subtitles` reads a Matroska or WebM film's text
subtitles -- SubRip, ASS, SSA, WebVTT -- into SRT markup for lane E's player
(design-decisions §1360). An MP4 film's subtitles are not found at all, and
subtitles stored as pictures (Blu-ray's, DVD's) are refused by name.

| What | Today | To do it |
|---|---|---|
| MP4's text tracks: `tx3g` (3GPP timed text, what `ffmpeg -c:s mov_text` and phones write), `wvtt` (WebVTT in MP4), `stpp` (TTML) | `Container::subtitles` returns none for MP4, so such a film has "no subtitles" | `gui/video/mp4` to give subtitle tracks and their samples (it gives video and sound), then a reader each: `tx3g`'s two-byte length, text and `styl`/`hlit` boxes as ffmpeg's `movtextdec` reads them; `wvtt`'s `vttc` boxes as ISO 14496-30 frames them |
| Pictures of text: Blu-ray's PGS, DVD's VobSub, DVB's | refused by name (`Error::SubtitleFormat`) | a decoder each (run-length bitmaps and palettes), and a picture cue type beside the text one |

**Where.** `gui/video/codec/src/subtitle.rs` and `subtitle/`;
`gui/video/codec/src/container.rs` (`subtitles`, `next_packet`);
`gui/video/mp4`.

**Was here, fixed 2026-10-05:** reading a film's subtitles read the whole film
from disk alongside the player, every video and sound packet's bytes to reach
a few kilobytes of cues. The Matroska demuxer now passes over the other
tracks' blocks unread, and a subtitle or sound reader reads ahead 1 KiB:
4.3% of a 1080p film read for its subtitles (design-decisions §1361); an MP4
film's sound reads 3.8% of it the same way.

**Was here, fixed 2026-10-05:** a WebVTT cue's settings (`line:0`,
`align:start position:0%`) were ignored, so a cue meant for the top or the
left came out at the bottom centre. They now place it, as `{\anN}` for the
third of the picture each way its text's anchor falls in, the anchor where
the specification lays the cue out (`webvtt::placement`): WebM's settings
from the block's second line, Matroska's from its addition, both held to
hand-checked answers in the fixtures.
