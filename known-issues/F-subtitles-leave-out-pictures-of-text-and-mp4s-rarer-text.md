### [F] Subtitles leave out MP4's rarer text formats, and DVD's custom colours -- 2026-10-05

**Status:** OPEN (lane F) -- limitations of `videocodec::Subtitles`, each
with what doing it would take.

**In short:** `videocodec::Subtitles` reads a film's text subtitles --
SubRip, ASS, SSA and WebVTT in Matroska and WebM, 3GPP timed text, WebVTT
and TTML in MP4 --
into SRT markup for lane E's player (design-decisions §1360), and Blu-ray's,
DVD's and DVB's pictures of text as images (§1362, §1363, §1365). One text
format MP4 can hold is not read -- television's CEA-608 captions -- nor
DVD's own colours where a rip gives them.

| What | Today | To do it |
|---|---|---|
| DVD subtitles' `custom colors` (VSFilter's recolouring, in the `.idx`) | ignored, as FFmpeg ignores it | the four colours the line gives in place of the palette's, `tridx` marking the transparent -- if a real rip needs it |
| CEA-608 captions in MP4 (`c608`) | a subtitle track of format `Other`, refused | a 608 decoder (byte pairs, roll-up and pop-on captions) |

**Where.** `gui/video/codec/src/subtitle.rs` and `subtitle/` (`vobsub.rs`);
`gui/video/codec/src/container.rs` (`subtitles`); `gui/video/mp4/src/track.rs`
(`subtitle_codec`).

**Was here, fixed 2026-10-05:** MP4's TTML (`stpp`, IMSC 1: what
broadcasters' DASH segments carry) was not read: a film with only it had
"no subtitles", and FFmpeg has no TTML decoder. It is read now as ttconv
reads it, each sample's document shown for its own stretch, each paragraph
a cue for each stretch it shows the same through, joined across samples
(`subtitle/ttml.rs`, `subtitle/xml.rs`, `subtitle/joined.rs`,
design-decisions §1368). MP4Box, the format's reference packager, loses
some subtitles on the way in -- see §1368 -- which every reader then
lacks, this one included.

**Was here, fixed 2026-10-05:** MP4's WebVTT (`wvtt`, ISO/IEC 14496-30,
what DASH and HLS segments carry) was not read: FFmpeg's table makes such a
track data, and FFmpeg reads none of it. It is read now, its samples' cues
-- cut wherever another cue begins or ends -- joined again and read as
WebM's are (`subtitle/isovtt.rs`, design-decisions §1367), held to the same
cues in WebM through MP4Box, the format's reference implementation.

**Was here, fixed 2026-10-05:** DVB's subtitles -- the pictures of text a
recording of digital television carries -- were refused by name. They now
come as images too: a receiver's, as FFmpeg draws them when told to read
the service's own pages and to give a region of no CLUT the standard's
default, with four departures where FFmpeg draws what a receiver would not
(`subtitle/dvb.rs`, design-decisions §1365). A DVB object coded as
characters rather than pixels is refused, as FFmpeg refuses it: no
broadcaster is known to send one.

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

**Was here, fixed 2026-10-05:** DVD's subtitles (VobSub) were refused, and
a film ripped from a DVD usually has no others. They now come as images
too, FFmpeg's pixels to the bit but where a DVD player shows otherwise --
each control sequence taking effect at its date, a transparent subpicture
clearing the screen (`subtitle/vobsub.rs`, design-decisions §1363).

**Was here, fixed 2026-10-05:** Blu-ray's subtitles (PGS), pictures of the
text, were refused, and a film ripped from a Blu-ray usually has no others.
They now come as images on the film's canvas, FFmpeg's pixels to the bit
but for crops, which are made as a Blu-ray player makes them; a seek reads
from the start of the epoch the time falls in (`subtitle/pgs.rs`,
design-decisions §1362).

**Was here, fixed 2026-10-05:** an MP4 film's subtitles were not found at
all. 3GPP timed text (`tx3g`, what ffmpeg, HandBrake and phones write) is
now read as ffmpeg's `mov_text` decoder reads it -- the default style, the
justification, style runs -- held to ffmpeg's SRT over four fixtures, three
of them written box by box (`subtitle/movtext.rs`).
