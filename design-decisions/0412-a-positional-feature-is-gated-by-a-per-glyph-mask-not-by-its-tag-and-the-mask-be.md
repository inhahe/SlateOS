## §412 — A positional feature is gated by a per-glyph mask, not by its tag; and the mask belongs to the (script, lookup) pair

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

### The problem

A face reaches Arabic's four positional forms through the features `isol`,
`init`, `medi` and `fina`. Those are ordinary GSUB type-1 lookups. Their
coverage, in every real face, is *every Arabic letter the face has* — that is
what makes them useful: give `fina` any letter and it hands back that letter's
final form.

That makes them unlike every feature the shaper had handled before. `liga` is
unconditional: if it matches, it applies, and asking "should this glyph be
eligible for `liga`?" has the answer "yes, always". `fina` is conditional on
something the lookup itself cannot express — whether *this* letter, in *this*
word, happens to be at the end of a join. Selecting `fina` by tag and running
it the way `liga` is run rewrites an entire word into final forms.

So the shaper's model — "a feature is a set of lookups; run them in order" —
is not expressive enough. It needs a second input: which glyphs each lookup is
allowed to touch.

### The decision

Three parts.

**1. A per-glyph mask, intersected with a per-lookup mask.** Each feature tag
in the shaper's list is assigned a bit. A lookup's mask is the union of the
bits of the tags that selected it. A glyph's mask is the seven unconditional
bits, plus at most one positional bit chosen by the joining pass. A lookup is
applied at a position only when `glyph.mask & lookup.mask != 0`.

This is HarfBuzz's design and it was adopted deliberately rather than
reinvented. The alternative considered was a per-glyph *feature set* — carry
the list of tags each glyph is eligible for and test membership. It is more
obvious to read and strictly worse to run: shaping is a nest of two loops over
lookups and positions, so the eligibility test is the innermost operation in
the crate. One `and` against a `u32` is the right cost for it. The mask's
limit is 32 features, and the shaper's list has 11.

**2. The mask belongs to the (script, lookup) pair, not to the lookup.** This
is the part that is easy to get wrong. Faces share lookups between features —
one lookup can be reached by Latin's `liga` and by Arabic's `fina`. If masks
were folded per lookup across the whole font, that lookup's mask would carry
the `liga` bit, and `liga` is always on in every glyph's mask, so the
positional gate would be defeated for every glyph in the run. `ByScript` in
`otl.rs` therefore stores `Vec<([u8; 4], Vec<(u16, u32)>)>` — per script, the
lookups it selects, each with the mask *for that script* — and folding
duplicates happens only within one script's selection.

**3. Alternate substitution resolves to the first alternate.** GSUB type 3
offers a list of candidates and expects the caller to pick one by feature
value. OpenType numbers alternates from 1 and a boolean feature that is on has
value 1, which selects `alternates[0]`. This is not a guess: HarfBuzz reaches
the same answer by packing the feature's value into the glyph mask and
indexing with it, which for an on-by-default feature is exactly 1. Type 3 had
to be implemented at all because Microsoft Uighur and its relatives write
their `init`/`medi`/`fina` as type 3 rather than type 1. A user-facing
alternate picker (`salt`, `ss01`…`ss20`, `aalt`) would need the value to be
plumbed through as data rather than assumed; that is not built.

Note that `AlternateSubstFormat1` and `MultipleSubstFormat1` are byte-for-byte
identical. Only the lookup type distinguishes "this glyph becomes these three
glyphs" from "this glyph becomes the first of these three candidates". The
dispatcher must therefore trust the declared type and never sniff the
subtable; a test (`a_subtable_is_read_as_the_type_its_lookup_declares`) pins
that.

### What was rejected

**A dedicated Arabic pass that rewrites glyph IDs directly**, consulting
`cmap` for each form, bypassing GSUB. This is how a naive shaper does it and
it cannot work: the four forms are not required to be at any predictable
place, faces disagree about which letters have which forms, and a face is
entitled to reach a form through a contextual lookup rather than a simple
substitution. The face's own lookups are the only correct source. The joining
pass's job is to say *which* feature applies where and then get out of the
way.

**Checking the mask at every matched position.** HarfBuzz tests the mask at
each component of a ligature and each glyph of a context's input. We test only
at the applied position. This is a real narrowness and it is filed as
`TD-FONT-CHECKS-FEATURE-MASKS-ONLY-AT-THE-APPLIED-POSITION`; it was left
because the check wants to live in the same skipping iterator that lookup
flags need, and building it twice is worse than building it once.

### The joining rule itself

A character joins to a neighbour only when *both* are willing, and the
neighbour is the nearest one that is not `Transparent` — so a combining mark
between two letters never breaks the join. "Before" and "after" are logical
order, not visual: this runs before any reordering, and reasoning about it in
visual order is how RTL shapers get written backwards. Join-causing (ZWJ) is
excluded from `shapes()` on purpose — it makes its neighbours join but has no
glyph of its own to put a form on.

`joining::forms` early-outs when no character in the run has a joining type
that shapes, so Latin pays one table lookup per character and nothing else.

### What the oracle says now

556 faces x 18 strings, against HarfBuzz:

| | before | after |
|---|---|---|
| agree | 9066 | 9023 |
| reversed (same glyphs, RTL order) | — | 44 |
| differ | 942 | 941 |

All 44 Arabic-capable faces moved from `differ` to `reversed`: the glyphs are
identical and only the order differs, because HarfBuzz reverses an RTL buffer
for the caller and we do not reorder at all
(`TD-FONT-DOES-NOT-REORDER-RIGHT-TO-LEFT-TEXT`). `Amiri-Bold.ttf` gives
`[55, 1700, 1428, 1745, 3113, 2420, 1633]` against HarfBuzz's
`[1633, 2420, 3113, 1745, 1428, 1700, 55]`. The sweep was taught to classify
that case separately for exactly this reason — burying it in `differ` would
have hidden the fact that the shaping agrees perfectly, which for Arabic is
the whole question.

The remaining 941 are the three deliberate normalization divergences of §410
(553 + 255 + 57 + 27), 43 mixed-script Arabic where HarfBuzz's whole-buffer
script guess is *less* correct than our itemizer, 5 Devanagari with no USE
shaper, and 1 Hebrew in SansSerifCollection.

**Code:** `gui/font/src/joining.rs`, `gui/font/src/joining_tables.rs`
(generated by `gui/font/tools/gen_joining_tables.py` from Unicode 16.0.0),
`ByScript::lookup_indices` and `for_script` in `gui/font/src/otl.rs`,
`FEATURES`/`form_mask`/`apply_lookup`/`apply_alternate` in
`gui/font/src/gsub.rs`, `ScaledFont::shape` in `gui/font/src/scaled.rs`.
Checked by `installed_fonts_join_arabic_letters` in
`gui/font/tests/host_fonts.rs` and by `gui/font/tools/harfbuzz_sweep.py`.
