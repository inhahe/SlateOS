## 1363. DVD subtitles are FFmpeg's pictures, but each control sequence takes effect at its date

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** a film ripped from a DVD carries its subtitles as pictures
(VobSub), and `videocodec::Subtitles` refused them. It now reads them, as
images like Blu-ray's (§1362), matching FFmpeg pixel for pixel -- except in
the few ways FFmpeg shows a DVD subtitle differently from a DVD player. A
DVD subtitle can change while it is on screen (fade, change colour, hide and
show again); FFmpeg shows only its first look. This shows each change when
the subtitle says it happens, as a DVD player does.

**What was decided.**

1. **The same cue of images as PGS**, `CueImage`s on the canvas the `.idx`
   setup gives (`size:`), FFmpeg's 720 by 576 where it gives none; colours
   straight from the setup's palette, alpha the nibble times 17.
2. **FFmpeg's `dvdsub` decoder is the oracle**, read off its output through
   sub2video, probe by probe, nothing translated (F-Q7): the two-bit
   run-length fields, even lines then odd; a picture's transparent edges
   cut away -- but a forced picture's, which FFmpeg keeps whole -- and a
   transparent pixel inside keeping its colour; an SPU whose codes do not
   fill its area, or whose fields lie past its data, changing nothing; no
   palette, and FFmpeg's grey ramps (one to four colours); no start
   command, and the picture shown from the block's time; no stop, and the
   block's duration ending it; times in FFmpeg's whole milliseconds,
   `(date << 10) / 90`.
3. **Where a DVD player shows otherwise, the player wins** (as §1360 has
   libass win, §1362 a Blu-ray player). FFmpeg draws an SPU's picture once,
   as its first control sequence leaves it, and keeps the last start and
   the last stop. The DVD specification has every control sequence take
   effect at its own date: a colour change or a fade (an alpha change)
   later in the SPU changes the picture then; a second start shows it again
   after a stop. And a new SPU replaces what is shown when it starts, even
   when every pixel of it is transparent (FFmpeg gives no subtitle for one,
   so the old picture stays up). `vobsub_dvd`'s answer is the generator's
   drawing at those times, FFmpeg's elsewhere.
4. **One model for both kinds of pictures.** A block of pictures gives
   timed *changes* -- from this time, these images, or none -- that the
   reader holds until the next block comes: what the last block had still
   to change before the next block's first change is made, the rest is
   superseded (a new SPU replaces an old one's later stop). A PGS display
   set is one change at its time; an SPU, one for each start, stop, colour
   or fade, and its block's duration a last clear where it never stops.
5. **A seek steps back one SPU**, an earlier one being able to show past a
   later one's time until that one starts.
6. **Kept as FFmpeg**: `custom colors` in the setup -- VSFilter's own
   recolouring, not the DVD format's -- ignored; damage changes nothing.

**Alternatives considered.**

- **FFmpeg's picture throughout** (one look per SPU, its last start and
  stop). *For:* the oracle untouched, no departures to draw. *Against:*
  fades and colour changes are how DVDs animate subtitles; FFmpeg shows a
  fading subtitle at full strength until it vanishes, and a subtitle shown
  twice only the second time.
- **VSFilter's `custom colors`.** *For:* VSFilter is the classic VobSub
  renderer and honours it. *Against:* it is a user's recolouring stored in
  the index by VobSubEdit, not part of what the disc shows; FFmpeg, and so
  every player built on it, ignores it. Revisit if a real rip needs it.
