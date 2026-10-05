## 1360. Subtitles are SRT markup in ffmpeg's form, saying what each format's own renderer shows

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous). The text form, SRT markup, was agreed
with lane E, whose player draws it (2026-10-05: "SRT markup is the right text
form because our SRT renderer takes it unchanged").

**In short:** a film's subtitles come in several formats: plain SubRip,
richly styled ASS (what fan translations and signs use), and WebVTT (the
web's). `videocodec::Subtitles` hands the player every cue, whatever its
format, in one form: SRT's small markup (italic, bold, a colour, a size,
top or bottom of the picture). ffmpeg's converter is the reference for how
that markup is written. But ffmpeg's ASS conversion often says something an
ASS player never shows: comments and drawing commands printed as text,
styling lost, the wrong style after a reset, sizes several times too large.
Where it does, this follows what the ASS player (libass) shows, and the same
for WebVTT and the WebVTT specification.

**What was decided.**

1. **One text form: SRT markup**, written as `ffmpeg -c:s srt` writes it --
   `<i>`, `<b>`, `<u>`, `<s>`, `<font>` with `color`, `face` and `size`,
   `{\anN}` for the placement, `\n` between lines. Lane E's player already
   reads SRT files, and every format here can be said in it, losing only
   what SRT cannot say (positions, rotation, karaoke, animation).
2. **ffmpeg is the oracle, as a black box.** Every fixture's answer is
   ffmpeg's SRT of the track, cue for cue (`tests/data/*.ffmpeg.srt`). Nothing
   is translated from ffmpeg's source, whose licence (LGPL) is an open
   question (F-Q7); its behaviour was read off its output, one probe a cue.
3. **Where ffmpeg's conversion contradicts the format's own renderer, the
   renderer wins**, and the fixture's answer is ffmpeg's with that cue
   changed (`generate_subtitle_fixtures.py`, `DEPARTURES`: 72 of 387 cues,
   each with its reason and checked by hand). The renderers:
   - **ASS and SSA: libass** (ISC licence; `ass_parse.c`, `ass_render.c` and
     `ass.c` read for the semantics, which follow VSFilter's). Every `{…}`
     is an override block (ffmpeg prints `{a comment}`, and stops reading a
     line at a block it cannot parse); drawings are not text; `\h` is a
     no-break space and `\n` a space unless the wrap style is 2; `{\r}`
     returns to the line's own style (ffmpeg: to "Default"); a value left
     out returns to the style's; `\b` takes weights; `\fs` takes fractions
     and relative steps; colours read as far as their digits go; the line's
     first `\an` decides its placement even against its style's; a style's
     strike-out is kept.
   - **Sizes are scaled to SRT's units.** An SRT size is 1/288 of the
     picture's height: ffmpeg's SubRip decoder, and so every player built on
     it, reads SRT into a 288-line script with 16 its normal size. An ASS
     size is in the script's own `PlayResY` lines. ffmpeg copies the number
     across, so a script laid out for 1080 lines -- most fansubs -- comes
     out at several times normal size. Here it is scaled.
   - **Styles closed under another are opened again** (all formats): to
     switch italic off under an underline, SRT must close both, and ffmpeg
     never reopens the underline.
   - **SubRip:** ffmpeg's reading, but a `</b>` out of order closes only
     the bold, as HTML and libass (through ffmpeg's own SubRip decoder)
     have it.
   - **WebVTT: its specification.** `&nbsp;` is U+00A0 (ffmpeg writes ASS's
     `\h`); `&quot;`, `&apos;` and numeric references are their characters
     (YouTube writes every apostrophe `&#39;`); an end tag that is not the
     innermost element's is ignored; ruby goes in parentheses after its base;
     a cue's settings (`line`, `position`, `size`, `align`), which ffmpeg
     ignores, place it: `{\anN}` for the third of the picture each way its
     text's anchor falls in, the anchor where the specification lays the
     cue out.
   - **3GPP timed text (MP4's `tx3g`): ffmpeg's `mov_text` reading**, held
     to it over files written box by box: the default style and the
     justification as the cue's style and place, style runs as what differs
     from the default, a run's end a return to it. Departures: its text is
     plain text, so `<i>`, `{\an8}` and `\N` in it are characters (ffmpeg
     passes them on as markup); UTF-16 text, which 3GPP allows, is read
     (ffmpeg drops the cue); an empty sample with a style box is no cue
     (ffmpeg makes an empty one).
4. **Text that SRT would read as markup is kept from it** by a U+2060 WORD
   JOINER, which shows nothing: after a `{` before a backslash, a backslash
   before `N`, `n` or `h`, and in WebVTT every `<` (which can only have been
   `&lt;`). ffmpeg does this for backslashes. For `{` it writes ASS's `\{{}`,
   which only an ASS renderer reads. One exception, as ffmpeg: HTML tags in
   an ASS line stay tags. libass would show them as text, but they are a
   converted SRT's slip far more often than text meant to be shown.

**Alternatives considered.**

- **ffmpeg's output exactly** (what lane E agreed to first). *For:* a single
  independent oracle for every cue, no hand-checked answers. *Against:* it
  shows a fansub's drawings and comments as garbage text and its dialogue at
  four times the size, loses styles, applies the wrong style after `{\r}`,
  and leaves a `<b>` open across cues after a `\b100`. The player would draw
  all of that.
- **A cue type of its own** (styled spans, a position) instead of SRT
  markup. *For:* no escaping, and room for what SRT cannot say. *Against:*
  lane E's renderer takes SRT, and a second form would need defining and
  drawing; ASS's positions and animation would still be lost without a full
  renderer.
- **Rendering ASS as libass does**, cues as pictures. *For:* everything ASS
  can show, positions and karaoke included. *Against:* a large port, and a
  different path through the player. It remains the way to complete
  fidelity, and does not conflict with this: text cues stay text.

**Consequences.** The fixtures' answers are ffmpeg's except where
`DEPARTURES` says, and `NAME.ffmpeg.srt` beside each `NAME.srt` keeps
ffmpeg's own for comparison. Lane E's renderer reads `{\anN}` anywhere in a
cue (it is written at the start), sizes in 1/288 of the picture with 16
normal, and U+2060 as nothing. What is not read yet:
`known-issues/F-subtitles-leave-out-pictures-of-text-and-mp4s-rarer-text.md`.
