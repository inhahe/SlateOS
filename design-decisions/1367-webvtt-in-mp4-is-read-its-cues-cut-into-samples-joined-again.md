## 1367. WebVTT in MP4 is read, its cues cut into samples joined again

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** web video split into pieces for streaming (DASH and HLS)
carries its subtitles as WebVTT inside MP4 files, and SlateOS's video
library found no subtitles in such a file -- FFmpeg, which it follows, does
not read them at all. It reads them now. The format chops a subtitle into
several pieces wherever another subtitle starts or stops while it is on
screen; the library glues the pieces back together, so a player gets each
subtitle once, whole, as it would from the same subtitles in a WebM file.

### How the format carries cues

ISO/IEC 14496-30 makes each sample a stretch of time and every cue showing
through all of it -- a `vttc` box each, holding the cue's identifier
(`iden`), settings (`sttg`) and text (`payl`) -- and a stretch with no cue a
sample saying so (`vtte`). A cue under two shorter ones is in five samples.

### What was decided

1. **A track FFmpeg makes data is subtitles here.** `mp4` keeps FFmpeg's
   classification -- `wvtt` is not in FFmpeg's subtitle table, so its track
   is data -- and `videocodec` offers a data track whose sample entry is
   `wvtt` as WebVTT. The demuxer stays FFmpeg's to the letter; the reader
   adds what FFmpeg leaves out.
2. **The pieces are joined again** (`subtitle/isovtt.rs`, `Joined`): a cue
   in a sample that begins where its last one ended, with the same
   identifier, settings and text, goes on; a cue in no sample after its
   last has ended there. Cues come out in the order they began, those
   beginning together in the order their first sample lists them -- the
   `.vtt`'s.
3. **Then each is a WebVTT cue as WebM's**, its text and settings read by
   the same rules, so MP4's WebVTT and WebM's cannot disagree.
4. **A seek** lands on the sample showing the time; a cue showing then
   begins at that sample, as far as the samples read after the seek say.

### Alternatives

| | For | Against |
|---|---|---|
| **Joining (chosen)** | A player gets each cue once, whole -- the same list as from WebM -- so a cue under another does not flicker or jump in the stacking each time the other begins or ends | One sample of waiting before a cue is known to have ended; a cue identical in identifier, settings and text to one ending just as it begins is taken for the same cue (the format cannot tell them apart) |
| A cue per piece | Nothing held over between samples | A cue under two others comes out as five; a player that stacks cues by order moves it each time |
| GPAC's own joining, as its `.vtt` export does it (`gf_webvtt_merge_cues`) | The reference implementation's answer | It walks the previous sample's cues in order and ends every cue it passes before the one matched, so two cues listed the other way round in the next sample are ended and begun again: a cue split for no reason in the source |

**Held to:** each fixture's `.vtt` is made into MP4 by MP4Box (GPAC, the
format's reference implementation, built from source in WSL -- it has no
Windows build that installs without an administrator) and into WebM by
ffmpeg; the MP4 is read to exactly the WebM's answer -- ffmpeg's cues, with
the placements already checked for WebVTT -- plain, with overlapping cues,
and fragmented as DASH segments are.
