## 1362. Blu-ray's subtitles are FFmpeg's pictures, to the bit, cropped as a player crops them

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** a film ripped from a Blu-ray usually carries its subtitles as
pictures of the text (PGS), not as text, and `videocodec::Subtitles`
refused them. It now reads them: each cue gives images (`Cue::images`) --
RGBA pixels placed on the picture the subtitles were made for -- which the
player scales with the film. What they show is what FFmpeg shows, pixel for
pixel and colour for colour, with one exception: where a subtitle asks for
only part of its picture to be shown (a "crop"), FFmpeg shows all of it and
a Blu-ray player shows the part; this shows the part.

**What was decided.**

1. **A cue grows images; text stays as it was.** `Cue` gains
   `images: Vec<CueImage>`, empty for text; a cue of pictures has empty
   `text`. Each `CueImage` carries its canvas's size (the film's, as the
   subtitles were made for it), its place on it, its pixels as straight
   RGBA, and whether it is *forced* (shown even with subtitles off). One
   cue type keeps one reading loop for the player, and the player draws
   whichever it is given.
2. **A picture's end is the next display set.** PGS gives no durations: a
   picture stays until a later display set replaces or clears it. The
   reader holds the picture on screen until the next set is read, then
   gives it with that set's time as its end; the last picture of a track
   ends at `i64::MAX`, the end of the film -- as FFmpeg's decoder marks a
   PGS subtitle's end "never". Pictures replaced at the very time they came
   were never on screen, and are no cue.
3. **FFmpeg is the oracle, held to its pixels.** Nothing is translated from
   FFmpeg's `pgssubdec` (LGPL, F-Q7); its rules were read off its output,
   probe by probe, through its `sub2video` -- which draws a subtitle track
   onto a transparent canvas, giving FFmpeg's decoded pixels exactly:
   - an epoch start *or* an acquisition point (either top bit of the
     composition state) forgets the objects and palettes;
   - a composition of no objects clears; one naming a palette that does
     not exist changes nothing (FFmpeg gives no subtitle); otherwise its
     first two objects show, a missing one passed over; a composition that
     cannot be read clears;
   - an epoch holds 8 palettes and 64 objects, counted by distinct id;
   - run-length codes fill the object in order, line ends moving nothing,
     and a run that would pass the end is skipped whole; an object the
     codes do not fill, or larger than the canvas, is dropped, taking the
     old object of its id with it;
   - colours are BT.709's studio range (BT.601's on a canvas 576 lines tall
     or less) in FFmpeg's 10-bit fixed point -- every one of 4096 random
     palette entries matched to the bit; exact arithmetic is off by one in
     7% of them.
   The fixtures (`pgs*.mkv`) are written by the generator segment by
   segment, and their answer is FFmpeg's picture at every change, as an
   MD5 of the canvas, with every subtitle and with the forced alone
   (`-forced_subs_only`). The generator also draws every well-formed
   fixture itself, from what it wrote, and stops unless FFmpeg agrees.
4. **Where FFmpeg contradicts a Blu-ray player, the player wins** (as
   §1360 has libass win over ffmpeg's ASS conversion). One place: a
   composition may crop its object, and FFmpeg reads the crop and shows the
   whole object anyway. The crate shows the crop, at the object's place;
   `pgs_cropped`'s answer is the generator's drawing at those three sets.
5. **A seek goes back to the epoch's start.** A display set in the middle
   of an epoch shows objects and palettes an earlier set defined, so a
   reader that starts at the set a seek lands on may show nothing where a
   picture is on screen. `Subtitles::seek` steps back, a display set at a
   time, to the epoch start or acquisition point before the time (at most
   64 sets), and reads on from there, passing over what ended before the
   time. Blu-ray discs usually begin an epoch at every subtitle, so this is
   one extra seek.
6. **Bounds of its own, for files that would take gigabytes**: a canvas at
   most 8192 pixels each way, an epoch's objects at most 32 MiB of pixels in
   all. FFmpeg would hold 64 objects of any size its canvas allows.

**Alternatives considered.**

- **A separate reader or cue type for pictures** (`PictureSubtitles`, a
  `CueContent` enum). *For:* a text player is untouched by fields it does
  not use. *Against:* the player opens one track and shows its cues; two
  types make it open and loop twice, and choose before it knows the format.
  The one field added is empty for text.
- **Exact BT.709 arithmetic** rather than FFmpeg's fixed point. *For:* it
  rounds the true value. *Against:* one in fourteen colours one off from
  the oracle, and tests that compare within a tolerance prove less; no
  viewer sees a difference of one in 255.
- **Reset on seek and read on, as a decoder joining the stream does**
  (FFmpeg's players). *For:* no extra seeks. *Against:* a seek into a
  palette fade, or onto a moved picture, shows nothing until the next
  epoch: wrong for exactly the seconds the user asked to see.
- **FFmpeg's whole object for a crop.** *For:* the oracle untouched.
  *Against:* a crop exists to hide the rest -- a picture revealed a line at
  a time, a scrolling credit -- and showing it all shows what the disc
  meant hidden.
