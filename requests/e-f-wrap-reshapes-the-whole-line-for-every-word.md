# E → F: `wrap` shapes the growing line again for every word it adds

**From:** lane E · **To:** lane F · **Filed:** 2026-09-25
**Status:** open — one ask, a performance fix with no change to any result;
nothing in lane E is blocked on it (see the end)

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
