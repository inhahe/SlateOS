## FIXED (2026-08-15, lane C) — three workspace test failures from real-glyph measurement, two of them real bugs

`text::measure`/`text::wrap` now measure actual glyph advances instead of
estimating from byte counts. Three lane-C tests failed as a result. Only one
was a stale test; the other two were genuine rendering bugs the old estimate
had been hiding.

**1. `weather::an_alert_card_grows_to_hold_its_description` — stale test.**
`card_h = (ALERT_BODY_TOP + body_height + 12.0).max(90.0)`, i.e. `52 + 18N`
floored at 90, so growth is only observable at N≥3 lines. `LONG_ALERT` used to
wrap to 4 lines and now wraps to 2 at `text_width = 828` (app width 900 minus
padding), so the test compared 90 against 90. Fixed by building the input by
construction — `"Secure loose objects outdoors. ".repeat(40)` — and asserting
first that it actually wraps past the floor (`drawn > 2`) so the growth check
can never again silently compare the floor to itself.

**2. `wordsearch` — real bug: the strikethrough rule and checkmark overran the
word they annotate.** A word in the list is drawn with
`max_width: Some(140.0)`, but the rule's extent and the checkmark's x were
placed from the *unclipped* `text::measure`. A word longer than the column got
a rule running out past the clip into the grid beside it. Fixed by naming the
clamp (`WORD_LIST_MAX_WIDTH`, `WORD_LIST_FONT_SIZE`) and applying it to the
measurement that positions the marks:
`text::measure(...).min(WORD_LIST_MAX_WIDTH)`. The old test asserted
`bold < word.len() as f32 * 8.0 + 1.0` — a byte-count literal, which is both
fragile and wrong for non-ASCII; replaced with three postcondition tests
(rule matches the word drawn beneath it; ÉLÉPHANT measures within 10% of
ELEPHANT, i.e. by character not by byte; a 45-char word's rule never leaves
the column).

**3. `tmux` — real bug: a terminal grid sized from a proportional face.**
`char_width()` was `text::digit_advance(...)`, the advance of `'0'` in the UI
face: 7.55px at 13px, while `'W'` in the same face is 13.08px. Glyphs overhung
their neighbours' cell backgrounds and the block cursor sat beside the
character it marks. The root cause was that **the toolkit had no way to ask
for a monospace face at all.** Fixed by building that dimension end to end —
`osfont::system::Family { Ui, Mono }` on the cache key, `text::measure_in` /
`cell_advance` / `line_height_in` / `ascent_in`, `RenderCommand::PushFont` /
`PopFont`, `guiremote` tags `0x0B`/`0x0C`, a `font_stack` in the compositor —
and pointing tmux at it. See `design-decisions.md` §413 for why the family is
scoped render state rather than a field on all 4570 `Text` construction sites.

**The pattern all three share**, and the rule that would have prevented them:
a threshold test whose threshold is a *literal* and whose input is *measured
by the environment* degrades silently long before it fails loudly. Assert a
postcondition of the function (`w <= box_w`; "the rule matches the word drawn
beneath it") or build the input by construction (`.repeat(40)`) — never encode
a fact about the host's installed fonts.

**Latent hazard this leaves.** `text::digit_advance` still exists and is still
the wrong call for any terminal-shaped view; its doc now says so and points at
`cell_advance`. Any other app that lays out a character grid should be checked
for it.
