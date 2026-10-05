## 1368. TTML in MP4 is read as ttconv reads it, each paragraph worked out where it changes

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** broadcasters and many streaming services send subtitles as
TTML (an XML format; IMSC 1 is its subtitle profile) inside MP4 files, and
SlateOS's video library found no subtitles in such a file. FFmpeg, which it
follows for most formats, cannot show them at all. It reads them now, the
way ttconv (the converter from the authors of the IMSC 1 reference player)
does. Each piece of the file is a complete TTML document showing only for
its own few seconds. The library glues a subtitle that runs across pieces
back into one. A piece that is not valid XML shows nothing, as in every
other player. A piece crafted to make a player spin for minutes is given up
on instead of being read.

### How the format carries subtitles

ISO/IEC 14496-30 makes each sample of a `stpp` track a whole TTML document,
shown only for the sample's stretch of time. A document times its
paragraphs (`p`) and the spans inside them, relative to their containers,
and places them in regions of the picture.

### What was decided

1. **ttconv is the reference** (pip `ttconv` 1.2.3, used as a library by
   the fixture generator): its timing, its region association and pruning,
   its styles (inline over referenced, inherited from parent and region) and
   its handling of white space, rule for rule. Two departures, where ttconv
   breaks TTML:
   - A parallel container that begins late lasts until its last child ends.
     ttconv sets the child's end, on the container's clock, against the
     container's begin on its parent's, and ends it early.
   - An element TTML does not allow where it is (a `p` in a `p`) is passed
     over alone. ttconv passes over it, every element after it, and its
     parent's own styles.
2. **Said in SRT as the other formats are** (§1360): bold, italic (oblique
   too), underline, line-through and colour (white being none); breaks as
   line breaks; hidden text not shown; `{\anN}` for the third of the
   picture each way the paragraph's anchor falls in.
3. **Each sample is read for its own stretch only.** A paragraph's cue is
   each stretch it shows the same through, joined across samples as
   WebVTT's pieces are (§1367), on the exact clock (fractions of a second),
   rounded to the nanosecond only when given.
4. **A sample that is not well-formed XML shows nothing, and is counted.**
   GPAC's MP4Box, the format's reference packager, writes such samples. It
   rewrites the document for each sample, and writes `&lt;` and `&amp;` in
   text back as bare `<` and `&`. So every subtitle with an ampersand that
   MP4Box packages is lost (GPAC git a23ae2d). Every reader rejects these
   samples: GPAC itself, reading its own file back ("Corrupted Data", the
   paragraph gone), ttconv, and the browsers' and ExoPlayer's XML parsers.
5. **What it costs is bounded by the sample's size.**
   - ttconv works the whole document out again at every moment anything in
     it begins or ends. For a document of many paragraphs that is its size
     times their number: about 10^11 steps for a 10 MB sample.
   - Here each paragraph is worked out only where it or something inside it
     changes, which says exactly what the whole-document reading says at
     every moment. A test holds the two together over 4000 random documents.
   - Styles and regions are looked up by a hash. White space is worked in
     one pass. Cues are joined in a logarithm a piece (`joined.rs`), which
     also replaced WebVTT's joining: that was quadratic too, in cues per
     sample and in cues ended while a long one shows.
   - A document that would still cost more than 32 steps per byte (with a
     floor of 4M steps) is given up on, and its sample is counted as
     damaged. Only a document made to be slow does that: one paragraph of
     thousands of spans, each beginning at its own time, whose every change
     shows the whole paragraph again. Subtitles cost about one step per
     byte; a film whose songs are karaoke timed syllable by syllable costs a
     few.
6. **Times are exact.** They are compared by continued fractions and
   rounded by long multiplication, so a document's 38-digit time cannot
   overflow into another time or into an order that is no order. A sort
   given an inconsistent order may panic.

### Alternatives

| | For | Against |
|---|---|---|
| **Strict XML (chosen)** | What GPAC, ttconv, imsc.js (dash.js), Shaka and ExoPlayer all do; XML 1.0 forbids reading on past a fatal error | An MP4Box-packaged subtitle with `&` or `<` is lost, as in every other player |
| Recovering a bare `<` or `&` as text, as HTML's parser does | The author's text shown; it cannot change the reading of a well-formed document | No reader does it, GPAC included, so a file that shows here shows nowhere else, and nobody fixes the packaging; more parser in front of hostile input |
| The whole document at every change, as ttconv does | ttconv's own procedure | Quadratic: a 10 MB sample takes minutes |
| A ceiling on paragraphs or elements per document | Simple | Rejects large real documents (a whole film in one sample) while still allowing quadratic cost below the ceiling |

**Held to:**
- **Each fixture's answer is ttconv's reading of the MP4's own samples**,
  each cut to its stretch and joined (`generate_subtitle_fixtures.py`,
  `ttml_cues`): styles, white space, escapes, colours and three regions;
  region association, including one region named inside another and an
  unknown one; no layout at all; and timing (offsets, frames, ticks,
  sequences, spans' own times, hours and minutes).
- **Some fixtures are written box by box**, each sample the whole document.
  Their reading is checked to be the document's own: in samples of 2 s and
  of 1.5 s, and in one sample. A seek gives what shows at the time.
- **Others are MP4Box's.** GPAC's splitter times each paragraph by its own
  `begin` and `end` alone, ignoring its containers, so the timing
  document's MP4Box file holds 4 of its 13 cues. That file is held to what
  its samples say, which tests that each sample shows only in its own
  stretch.
