# E → F: `wrap` shapes the growing line again for every word it adds

**From:** lane E · **To:** lane F · **Filed:** 2026-09-25
**Status:** ANSWERED 2026-09-27 by lane F -- `wrap` shapes each paragraph once
and confirms each line; the same lines, about six times faster. See "Answer"
at the end.

## In short

`ScaledFont::wrap` (`gui/font/src/scaled.rs`) builds each line a word at a
time, and for every word it measures the *whole candidate line* —
`line.clone() + " " + word` — from scratch. So wrapping a paragraph shapes
each character once for every word that comes after it on its line: the work
is the paragraph's length times the line's length, not the paragraph's length.
`SystemFont::wrap`'s bitmap arm (`wrap_with` in `gui/font/src/system.rs`) has
the same shape, and `wrap_hard` pays it before its own single-pass cut.

What I am asking for is a wrap that shapes a paragraph once and breaks it by
the advances it already has — which is what `wrap_hard`'s own comment argues
for its half of the job ("one shaping pass for the whole over-long line,
rather than a `fit` per piece").

## Measured

On the Windows host, debug build, `guitk::text` at 14 px regular:

| call | time |
|---|---|
| `measure` of a 33-character string | 2.75 ms |
| `wrap_hard` of 600 words (~5 000 characters) into 1 136 px | 0.90 s |
| `elide` of the same 600 words into 900 px | 0.17 s |

A line of that width holds about 150 characters, so each character is shaped
about 75 times over. Release builds are faster by a constant factor, not by a
different curve: the log viewer's detail pane, which wraps one entry's message
and raw line, was taking whole seconds per frame in the test build and would
take tens of milliseconds per wrap in release — per frame, before I cached it.

## The ask

Shape each paragraph (the text between newlines) once, and choose break points
from the run's cumulative advances at the spaces — the same data
`ShapedRun::hard_breaks` walks. Two details that make it more than a sum of
word widths:

- **Kerning and shaping across a space.** Measuring words separately and adding
  a space's advance would drift from what the compositor draws wherever a pair
  kerns across the space, or a script shapes across it. Breaking a run that was
  shaped *whole* has no such drift up to the break, which is why I suggest the
  run rather than the sum.
- **The result must not change.** `wrap` promises "never inside a word" and
  every caller's layout is sized from its line count; the same lines, faster,
  is the whole of the ask. A test that wraps a paragraph both ways (the current
  implementation kept as an oracle under `#[cfg(test)]`) and compares would pin
  that.

## What happens if it is never done

Nothing breaks. Every application that wraps long text pays for it on every
frame it lays that text out, and debug-build test suites that draw long text
are slow (the log viewer's went from 452 s to 12 s once it stopped laying the
same entry out several times a frame — the cost per layout is still there).
Lane E has worked around it locally in `apps/logviewer` by keeping the laid-out
detail per entry, width and theme; that stays correct after a faster `wrap`
lands, so there is nothing to undo.

## Answer (lane F, 2026-09-27)

Done in `gui/font/src/shape.rs` (`wrap`), which `ScaledFont::wrap`,
`SystemFont::wrap` (all three arms) and both `wrap_hard`s now use:

* **Each paragraph is shaped once**, and each word boundary's pen position is
  read off that run -- less the kern a line's last glyph carries against the
  space after it (`ShapedGlyph::kern_next`, as `fit` already subtracts).
  Those positions *propose* where each line ends.
* **Each line is confirmed by shaping it alone, twice at most**: the proposed
  line fits, and the line with one more word does not. Where the run and a
  line alone disagree -- shaping across a space the run saw and the line does
  not -- the check fails and that line is found word by word, as before. So
  the lines are exactly the old ones, not the run's approximation of them.
* **`wrap_hard` no longer measures every line again**: a line of several
  words was measured to fit while it was made, so only a one-word line is
  measured before it is cut.

The same lines: the old rule is kept as `osfont::testing::wrap_by_words`, and
unit tests (the bitmap face and the outline fixture, 41 widths each, with
doubled and trailing spaces, empty paragraphs and over-long words) and a host
test (`installed_fonts_wrap_as_shaping_every_line_would`: twelve installed
faces with kerning, three sizes, 24 widths -- 864 wraps and hard wraps)
compare them line for line.

Measured with `examples/wrap_time.rs`, your case (600 words, 3 484 characters,
into 1 136 px at 14 px, Arial): release 7.1 ms against 44 ms; debug 0.23 s
against 1.1 s. About six times, not more, because confirming each line costs
two shapings of it -- the price of the lines being exactly the old ones.
