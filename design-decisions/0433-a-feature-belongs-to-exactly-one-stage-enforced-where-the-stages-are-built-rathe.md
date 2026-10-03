## §433 — A feature belongs to exactly one stage, enforced where the stages are built rather than where they are applied

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A font's shaping rules are grouped into named "features"
(`ccmp` composes characters, `blwf` picks the below-the-line form of a letter,
and so on), and the shaper runs them in a fixed sequence of stages. Both the
Indic and the Khmer shaper were listing some features in an early stage and
then *again* in a final catch-all stage, so those features ran twice. Usually
that changes nothing — running a rule on its own output is normally a no-op —
but it is not what HarfBuzz does, and the second application can rewrite a
glyph the first one already produced. The decision is where to fix it: in the
code that applies stages, or in the code that builds them.

### How it was found, and why that matters

Not by reading. The duplication had been in `indic_shape.rs` since that shaper
was written, and every one of the 556 faces installed on this host agreed with
HarfBuzz anyway, because a real font's lookups are overwhelmingly idempotent:
substituting a below-base form for a glyph that is already a below-base form
matches nothing the second time. It took the Khmer probe font of §431 — where
every feature's lookup appends a marker glyph, and so is *deliberately* not
idempotent — to make the second application visible, as a doubled marker.

That is the §431 argument arriving a second time from a different direction:
the host had been reporting agreement about a code path it could not exercise,
and the bug it hid was not in the shaper being written but in one that had
been shipping for weeks.

### The two places it could be fixed

**In `gsub::apply_stages`** — track which feature bits have already run and
mask them out of later stages. This is closest to what HarfBuzz literally
does: `hb_ot_map_builder_t::compile` sorts the collected features by tag,
merges duplicates, and keeps `hb_min` of the two stage numbers.

**In each shaper's stage list** — build the stages so they are disjoint by
construction, by masking the earlier stages out of the final catch-all.

The second was chosen, for two reasons that pull the same way:

* **`apply_stages` has a legitimate reason to run the same lookup twice.**
  Deduplication belongs to *features*, not lookups. Two different features
  routinely point at one lookup, and when they are in different stages that
  lookup genuinely must run twice, with different masks — that is how a
  feature that is on for one syllable and off for the next is expressed at
  all. A dedup living in `apply_stages` would be one refactor away from being
  written against lookups instead of features, and that version would be
  silently wrong in a way no host face would reveal either.
* **A shaper that names a feature twice has a bug in its stage list**, and the
  stage list is where a reader looks to answer "when does `blwf` run?". Fixing
  it downstream leaves the wrong answer written in the place people read.

So `khmer.rs` and `indic_shape.rs` each grew a `stages()` function returning
the stage masks, with the final stage computed as
`ALL_FEATURES & !already_staged & !liga`. Extracting it from `shape` was the
point of the exercise rather than tidiness: a `stages()` that returns a value
can be asserted about, and four tests now pin the invariant — the stages are
pairwise disjoint, their union is every feature except `liga`, the first stage
is exactly the basic set, and the global features fall in the last stage only.
A comment saying the same thing is deletable by anyone who thinks it is
redundant.

*Cost of the choice:* the invariant is restated once per shaper rather than
enforced once centrally, so a fourth shaper can reintroduce it. That is what
the tests are for, and a shaper without the corresponding test is a shaper
whose stage list nobody checked. *Benefit:* `apply_stages` keeps the ability
to run one lookup under two features, which the alternative would have put at
risk, and the Khmer probe went from **1 agreement of 45 to 43** — measured by
putting the duplication back and re-running, not estimated. That figure is the
other half of the point: against a face that can object, a feature applied
twice is not a subtle difference, it is almost every string. Against the 556
faces installed here it was none of them.

**Where:** `gui/font/src/khmer.rs` (`stages`, `shape`, and the two stage
tests), `gui/font/src/indic_shape.rs` (the same four), `gui/font/src/gsub.rs`
(`apply_stages`, deliberately unchanged), `gui/font/tools/gen_khmer_probe.py`
(the oracle that exposed it), `design-decisions.md` §431.
