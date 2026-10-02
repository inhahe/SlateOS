## §427 — Text that does not fit carries an overflow policy on the draw command, and the compositor draws the ellipsis

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Operator (answering `open-questions.md` Q45 — "q45: a."; Claude
raised the question and recommended A)
**Zone:** gui-core, gui-toolkit, apps

**In short.** When a piece of text is too wide for the space it was given, we
currently just stop drawing it — no "…", no mark of any kind. So a label reading
`Gateway 192.168.1.1 res` looks like a complete sentence rather than a truncated
one, and a reader has no way to tell that anything was cut. The fix is to make
every text-drawing instruction say up front what should happen when the text
does not fit — either cut it silently or end it with "…" — so that the question
can no longer be left unanswered by accident. The cost is that every place in
the codebase that draws text has to be edited to say which it wants.

**Context.** `RenderCommand::Text` carries an optional `max_width`. The
compositor honours it in `draw_text` by walking glyphs and breaking before the
first one that would cross the limit. Nothing is drawn to mark the break. A
caller who wants the cut marked has to call `text::elide` beforehand — which
measures the string to find the cut point, and then the compositor measures it
again while drawing it, answering the same question twice with two
implementations that can disagree.

The result is the failure mode in `known-issues.md` →
`TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED`: well over a hundred single-line labels
across `gui/**` and `apps/**` pass `max_width` without eliding. Most are safe
only because their values are short and app-authored. The ones that bite carry
user or network data — file names, SSIDs, error strings, host names — where a
plausible-looking truncation is indistinguishable from the real value.

**Options.**

1. **`overflow: TextOverflow` (`Clip` | `Ellipsis`) as a field on
   `RenderCommand::Text`; the compositor draws the ellipsis.** *What changes:*
   text cut by `max_width` ends in "…" wherever a caller asks for it, and the
   ellipsis is placed by the party that knows exactly where the glyphs ran out.
   *Cost:* Rust has no per-field default in a struct variant, so this edits
   **every** construction of `Text` in the tree.
2. **A second variant, `RenderCommand::ElidedText`.** *What changes:* the same
   visible outcome, with no edit to existing call sites. *Cost:* every renderer,
   every test and every match on `RenderCommand` splits an arm forever to encode
   one boolean.
3. **A builder — `Text::new(..).ellipsis()`.** *What changes:* the same outcome,
   opt-in. *Cost:* the struct-literal form stays available and stays wrong, so
   the next label someone writes still has the bug.
4. **Sweep `text::elide` across the call sites that need it.** *What changes:*
   today's hundred-odd bad labels get fixed. *Cost:* nothing prevents the
   hundred-and-first, and the double-measurement stays.

**The decision: option 1.** It is the only option that makes the mistake
*unrepresentable* — after it, a `Text` command cannot exist without having
answered "and what if it doesn't fit?". The operator was told the churn was
several hundred sites and chose A anyway, on exactly that ground. (Measured
afterwards: **4517 `RenderCommand::Text {` sites across 208 files**, well above
the estimate. That does not reopen the decision — the decision was about
representability, not diff size — but it does mean the edit is *scripted*, not
made by hand.)

**Execution constraint, and it is load-bearing.** This lands as **its own commit
with nothing else in flight.** A four-thousand-site mechanical diff entangled
with real work cannot be separated afterwards; that is precisely the trap §310
(the repo-wide rustfmt) exists to document, and it cost a revert-and-redo cycle
in `posix` when it happened there.

**A sub-decision left to Claude, recorded here so it can be overruled.** For the
sites that pass `max_width: None`, the choice is vacuous and they get `Clip`.
For the sites that *do* set a `max_width`, the mechanical translation would be
`Clip` — that preserves today's behaviour exactly. It is nonetheless the wrong
default: today's behaviour *is* the reported bug, and a scripted sweep that
faithfully preserves a bug at four thousand sites has done nothing. Those sites
default to **`Ellipsis`**. The consequence is that some labels which currently
fill their box to the last pixel will end in "…" one glyph earlier; that is the
intended change, not a regression. Sites where clipping is genuinely correct —
a progress bar's fill, a decorative rule — are those that should be
individually set back to `Clip` afterwards, because they are the rare case and
can be argued for one at a time.

**Where it lands.** `gui/toolkit/src/render.rs` (`RenderCommand::Text`, the
`RenderTree::text()` helper), `gui/compositor/src/main.rs` (`draw_text` — the
`break` at the limit becomes the place the ellipsis is drawn),
`gui/toolkit/src/text.rs` (`elide` / `elide_start`, which now overlap the
compositor's job and need reconciling rather than deleting — they still serve
callers who need the *string*, not the pixels), and every `max_width: Some(..)`
in `gui/**` and `apps/**`. Closes `known-issues.md` →
`TD-GUI-CLIPPED-TEXT-IS-NOT-MARKED`.
