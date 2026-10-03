## TD-FONT-CANNOT-BE-TOLD-A-PARAGRAPH-DIRECTION

**Status: FIXED** (2026-08-16, lane C). `ScaledFont::shape_with(text, lang,
base)` is the new full form; `shape` and `shape_lang` are it with defaults.
Three departures from the sketch below, each because the sketch was wrong about
something:

* **It carries the language too**, rather than being `shape_with(text, base)`
  beside `shape_lang(text, lang)`. Two orthogonal knobs on two methods is a
  matrix — the next caller that knows both would have needed a fourth method.
  One full form and two named defaults does not grow.
* **The left-to-right fast path had to be gated on the base.** `byte_levels`
  returned early for any string `bidi::is_trivially_ltr` accepted, and that
  test is a claim about the *answer* — "every level comes out even" — which
  holds only while the paragraph's own level is even. Under `Base::Rtl` the
  same string resolves to level 2 inside a level-1 paragraph. Left alone, the
  fast path would have silently discarded the base the caller had just given,
  which is the single thing the function must not do.
* **The entry's own example does not demonstrate the bug.** `"(123)"` under
  `Base::Rtl` renders *identically* to the `Auto` answer: rule L4 mirrors both
  brackets and rule L2 swaps their positions, and the two cancel. That is
  correct behaviour — a number in a Hebrew sentence still reads left to right,
  brackets and all. The case that does differ is an unbalanced or asymmetric
  one, so the regression test uses `"(a"`, which draws as `(a` under `Auto` and
  as `a)` under `Rtl`.

No caller passes anything but the default yet; the widgets that should are
`TD-GUI-WIDGET-CARETS-ARE-NOT-BIDIRECTIONAL`'s subject, and the layout stage
that knows a container's direction is the one that will supply it.

Verified: 719 lib tests (3 new on `byte_levels`), 19 host-font tests (one new,
`a_paragraph_direction_can_be_given_and_changes_the_answer`, checking 547 of
this host's faces), bidi conformance suite green, `clippy --all-targets` clean.
See `design-decisions.md` §443.

**What.** `ScaledFont::shape` resolves the bidi base direction with
`Base::Auto` — UAX #9 rule P2, "the first strong character decides" — and has
no parameter with which a caller could say otherwise.

**Symptom.** A string with no strong character at all, or one whose first
strong character is the wrong way round for its context, lays out against its
container. `"(123)"` in a right-to-left paragraph is the standard example: P2
finds nothing strong, defaults to left-to-right, and the parentheses come out
mirrored the wrong way. A Hebrew UI label reading `"OK"` is the same problem
from the other side. Nothing in the sweep catches this, because HarfBuzz's
`guess_segment_properties` makes exactly the same guess.

**Why it is filed rather than fixed.** The fix is a signature change, and the
right signature depends on a caller that does not exist yet. `shape(&str)` is
called from the toolkit, the compositor and two apps; adding a `Base` argument
to all of them before anything can *supply* one usefully would be churn that
has to be redone when the paragraph model above it lands. The layout stage
that knows the container's direction is the one that should pass it.

**Proper fix.** `shape_with(&self, text: &str, base: Base)` beside the current
`shape`, which keeps calling it with `Base::Auto`. `bidi::Base` is already the
public enum with the three cases (`Auto`, `Ltr`, `Rtl`), and `byte_levels` in
`scaled.rs` already takes the base as a value — it is threaded through, just
not exposed.

**Where.** `gui/font/src/scaled.rs` — `shape` and the `byte_levels` helper
below it, whose doc comment names this entry.
