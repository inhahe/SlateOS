## 1365. DVB's subtitles are a receiver's, as FFmpeg draws them, with four departures

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** recordings of digital television (DVB broadcasts kept as
`.mkv`) carry their subtitles as pictures (`S_DVBSUB`), which
`videocodec::Subtitles` refused. It now reads them into the same cues of
images as Blu-ray's and DVD's. What they show is what FFmpeg's `dvbsub`
decoder draws when told to behave as a television receiver does (two of
its own options), except in four places where FFmpeg draws something a
receiver would not; there the receiver wins, as it did for DVD and
Blu-ray (§1362, §1363).

**What was decided.**

1. **FFmpeg is the oracle, told to read as a receiver.** Its rules were read
   off its output (sub2video, as for PGS and VobSub), probe by probe, and
   held to it by the fixtures; where a damaged stream's handling turns on
   how far a read runs past a field -- what no probe shows until it is
   asked -- FFmpeg's `dvbsubdec.c` (at the pinned git 9b7439c31b) said what
   to probe. The code is this crate's own, written to those rules, not a
   translation. Two of FFmpeg's options make it a receiver, and are the
   oracle's:
   - `-dvb_substream 0`: read only the subtitle service the track names --
     its composition page and ancillary page, from the CodecPrivate.
     FFmpeg's default reads every page's segments, drawing two services (two
     languages carried on one stream) over each other.
   - `-compute_clut 0`: a region whose CLUT was never sent shows through the
     standard's default CLUT (EN 300 743 section 10). FFmpeg's default
     invents a grey-green ramp from the picture's own pixel values instead.
2. **The rules** (each in `subtitle/dvb.rs`'s documentation, held to
   fixtures): pages of the held version ignored; acquisition points and mode
   changes forgetting regions, objects and CLUTs; regions keeping their
   pixels between display sets, filled when asked or resized; object data
   drawn when it comes into every region that places it; CLUTs and display
   definitions taken only in new versions; the display window moving the
   regions; a display set shown at its end segment, or at a block's end
   when it holds a page, a region and an object; the page timeout clearing;
   and damage (a block of six bytes or fewer, a segment past its block, an
   object outside its region, data for an object no region places,
   characters for pixels, a region of no size) showing nothing new, as
   FFmpeg's.
3. **Four departures**, each a fixture time where the crate's answer is the
   generator's own drawing (`dvb_receiver.mkv`):
   - *A CLUT entry marked for several tables goes into each*, as the
     standard has it. FFmpeg puts it into the first only, so a 4-bit
     region keeps the default colours for entries a stream defined.
   - *A region shows from its composition*, its background before any
     object is drawn in it. FFmpeg shows a region only once an object has
     been drawn into it.
   - *The non-modifying colour leaves its pixels, and the next in theirs.*
     FFmpeg counts a pixel of it without moving past its place, so the
     rest of the line is drawn that many pixels to the left: a bug.
   - *Of several display sets in one block, the last shows.* FFmpeg keeps
     the first that shows anything and refuses the rest ("Different Version
     of Segment asked Twice"), though it reads their segments.
4. **Two bounds of this reader's own**, for streams FFmpeg would read
   without end: a page's regions hold four times FFmpeg's largest region in
   all (its own bound is per region), and place 1024 objects in all (one
   object's data is drawn once for each placement).
5. **A seek goes back to where the pictures begin afresh** -- an
   acquisition point or a mode change, up to 64 display sets back -- as for
   Blu-ray's epochs.

**Alternatives.**

- *FFmpeg's default options as the answer.* Exact to what `ffmpeg -i` does,
  but wrong on the two streams where it matters: a stream carrying two
  services, and a stream that relies on the default CLUT.
- *FFmpeg everywhere, departures included.* The four are FFmpeg's bugs or
  shortcuts against the standard, on well-formed streams; the rule for
  DVD and Blu-ray (§1362, §1363) is that the player wins there.
- *Characters drawn* (objects coded as character strings, which FFmpeg
  refuses): a receiver would draw them from its own font. No broadcaster
  is known to send them; refused, as FFmpeg refuses them, until one does.
