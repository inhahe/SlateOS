## §409 — Shape every installed face against HarfBuzz, and set limits from what that measures

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

### Context

Our GSUB implementation had 222 unit tests, mutation-tested guards, and
13 host-font tests over 556 real faces. All green. It was also silently
dropping most of the lookups in 61 of the 365 installed faces that have a
`GSUB`.

The unit tests could not catch it because they build their own fonts, and
a font written to test a feature has exactly the lookups that feature
needs. The host tests could not catch it because *returning the input
unchanged is a legal answer*: a face with no ligature for `fi` correctly
gives back two glyphs, so "gave back two glyphs" cannot distinguish a
face that has no ligature from a face whose ligature we failed to reach.
Every assertion we had was consistent with the bug.

### Decision

Use HarfBuzz as an independent oracle: shape a fixed corpus through every
installed face with both implementations and compare glyph ids. This
meant installing `uharfbuzz` on the development host.

Result over 556 faces and 13 strings: **6426 agree, 324 differ**, and the
324 fall into exactly three classes, of which one was ours:

- **288 — `e` + U+0301.** HarfBuzz runs its own Unicode normalizer
  (`hb-ot-shape-normalize`) before GSUB and composes the pair to a single
  precomposed glyph. That is not GSUB and not a fault in our tables; it
  is a genuine missing stage, now the next shaping item on the roadmap.
- **33 — Amiri and FiraCode** losing Latin ligatures and contextual
  alternates entirely. A real bug: see below.
- **3 — Calibri `1/2`.** A one-glyph disagreement in fraction handling,
  logged in `known-issues.md` rather than guessed at.

### The bug it found, and the rule it produced

`otl::MAX_SUBTABLES` was 64, shared across every lookup a face's features
reach, and its doc comment justified that as "real fonts use single
digits; the largest seen on the development host is 4." That number was
per *lookup*. Summed across the lookups our features reach, 61 installed
faces exceed 64 and the worst declares 1874.

Amiri lists its large Arabic feature set before its Latin `liga`, so the
budget was exhausted before the ligature lookup was reached and `office`
came back as six separate glyphs. FiraCode never reached the `calt` that
shortens `f` before `i`.

The cap itself is kept — the cost of a shape is the run length times this
number, so a hostile font must not be able to set it — but the value is
now measured and the measurement is recorded beside it:

| measure | worst face | count |
|---|---|---|
| subtables in one lookup | SansSerifCollection | 675 |
| lookups reached | SansSerifCollection | 256 |
| subtables in total | SansSerifCollection | 1874 |
| runner-up total | JetBrains Mono | 768 |

8192 is a little over four times the worst real face. Exceeding it still
shapes with the lookups found rather than rejecting the font: a slightly
wrong ligature is a better failure than a blank page.

**The general rule this establishes: a limit whose justification is a
glance is a bug waiting to happen, and the failure mode of a limit set
too low is silence.** Any budget, cap or depth in this crate should carry
the measurement that set it, so the next person to touch it knows what it
protects against and what it must clear.

### Alternatives considered

- **Derive the budget from the table's byte length** (a subtable offset
  costs two bytes, so the count is bounded by `len/2`). Attractive
  because it cannot be outgrown, but it makes the worst case scale with
  font size, which is precisely what the cap exists to stop.
- **Reject a face that exceeds the budget.** Rejected: a face that is
  merely large is not hostile, and a missing font is a worse
  user-visible failure than a missing ligature.
- **Commit the HarfBuzz comparison as a test.** Rejected for now: it
  needs Python and `uharfbuzz` on the host, which the Rust test suite
  cannot assume. What is committed instead is
  `installed_fonts_reach_lookups_past_the_subtable_budget`, which names
  specific faces whose answers are known to live deep. The choice of
  faces is the content of that test: JetBrains Mono declares more
  subtables than FiraCode and is deliberately *not* listed, because
  HarfBuzz shapes its `fi` unchanged too — a deep face only makes a
  witness when something deep applies to it.

### Where this lives

`MAX_SUBTABLES` and its measurement table in `gui/font/src/otl.rs`;
`installed_fonts_reach_lookups_past_the_subtable_budget` in
`gui/font/tests/host_fonts.rs`.
