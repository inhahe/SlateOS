## §406 — GSUB is an ordered list of lookups, not a bag of subtables; single substitution and `ccmp` before contextual (GSUB 5/6)

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

**Context.** §402 read GSUB for one purpose — find LookupType 4 anywhere in
`liga`/`rlig` and join glyphs — so it flattened every matching lookup into one
list of subtable offsets and walked that. That is adequate for ligatures alone
and wrong for anything else, which the next step needed.

**Decision, part one: a lookup is the unit of application.** `otl::Lookup`
keeps a lookup's type together with its own subtable offsets, and
`Substitutions::apply` runs each lookup across the *whole* glyph buffer before
starting the next. Within one lookup, subtables are tried in font order and the
first match wins; a glyph one subtable substituted is not re-offered to the
rest of that lookup.

This is not a refactor for tidiness — it is the only mechanism that makes
`ccmp` reliably precede `liga`. A font lists its lookups in the order it wants
them applied, so what lookup 1 substitutes is what lookup 2 sees. Flattening
subtables into one list destroys that boundary: `ccmp` mapping A→B and a
ligature covering B but not A would find nothing to join in a single flat pass.
Two tests pin the semantics in both directions —
`an_earlier_lookup_feeds_a_later_one` and
`a_later_lookup_does_not_feed_an_earlier_one`.

*Looping to a fixpoint was rejected*, and the second of those tests is why: a
pass that repeated until nothing changed would ligate in the second case too,
which is the font asking for the opposite. It also would not terminate on a
font whose lookups feed each other cyclically. One ordered pass is both correct
and total.

Callers with genuinely one lookup type and no ordering to preserve — pair
kerning, mark attachment — keep the flat list through `feature_subtables`,
which is now a thin wrapper over `feature_lookups`. They were not touched.

**Decision, part two: type 1 and `ccmp` now, types 5/6 later.** The roadmap
bullet named contextual substitution (GSUB 5/6) as the next unblocked step.
That is out of order: 5 and 6 work by invoking *other* lookups by index at a
matched position, so a general "apply lookup N here" mechanism plus the simple
types it dispatches to are a strict prerequisite, not a parallel feature. This
commit therefore builds the ordered lookup list and LookupType 1 (Single
Substitution, both formats) and adds `ccmp` to the default-on feature set;
5/6 become implementable rather than blocked.

**What is deliberately still out, each for its own reason.**

- *Type 2 (Multiple).* One glyph becomes several, so the run grows and two
  glyphs share a cluster — which `shape.rs` currently assumes cannot happen.
  `ShapedRun::x_of` and `fit_end` both give wrong answers under it (traced:
  with clusters `[0,0,1]`, `x_of(0)` returns the first glyph's advance instead
  of zero). That is a change to `shape.rs`'s invariants with its own test
  story, not a change to `gsub.rs`, and is the next commit rather than this
  one.
- *Type 3 (Alternate).* Picks by an alternate index that only a per-run feature
  list can supply, and there is none. It has no default-on caller, so
  implementing it would add an unreachable branch.
- *`locl`.* On by default in every shaper, but *language*-specific. Applying it
  without knowing the run's language hands every reader some other locale's
  letterforms, which is worse than not applying it.

**Consequence, and the known hole.** Widening the read from `liga`-only to
"any applicable substitution" makes the existing no-script-selection limitation
bite for the first time: the feature walk takes every script's features, and
`ccmp` is precisely where a script puts its normalisation rules. On the
development host this is visible — `ebrima.ttf` substitutes the *space* glyph
from an African script's `ccmp`. Logged as
`TD-GSUB-APPLIES-EVERY-SCRIPTS-FEATURES` and verified against an independent
Python parse of the table, so it is our *selection* that is wrong, not our
*parsing*. Two new host-font tests bound the damage:
`installed_fonts_leave_plain_latin_alone` asserts the *proportion* of faces
that alter Latin prose stays tiny (a parser fault hits fonts in bulk, a genuine
rule hits a handful), and `installed_fonts_leave_a_tab_alone` asserts the tab
survives.

**Decision, part three: a substitution may not reach across a tab.** A tab is
carried through shaping as the space glyph, but it is a layout decision wearing
a glyph's clothes. A GSUB lookup handed a run matches anywhere in it, so the
only way to express a boundary is to not show the lookup across one:
`ScaledFont::substitute_between_tabs` substitutes each stretch separately.
Without it a lookup covering the space replaces the tab (Ebrima again), or a
ligature swallows it and the tab flags stop lining up with the glyphs, after
which every advance past that point is charged to the wrong glyph. The same
mechanism is what style changes and bidi run edges will use: `Face::substitute`
takes a run, and the caller decides what one run is.

**Where it lives.** `otl::{Lookup, feature_lookups, read_lookup,
lookup_indices}` in `gui/font/src/otl.rs`; `gsub::{SubGlyph, Substitutions,
apply_single, single_at, apply_ligature}` in `gui/font/src/gsub.rs`;
`Face::{substitute, has_substitutions}` in `gui/font/src/sfnt.rs`;
`ScaledFont::{shape, substitute_between_tabs}` in `gui/font/src/scaled.rs`.
