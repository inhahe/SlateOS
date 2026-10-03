## TD-FONT-A-ZEROED-MARK-IS-NOT-PARKED-ON-ITS-BASE -- 2026-09-17

**In short:** on a font that cannot draw a script at all, HarfBuzz tucks that
script's combining marks back onto the letter they belong to and we leave them
where the pen was. Every glyph involved is a missing-glyph box, so nothing
looks different; what differs is where the boxes sit. Forty cases, all in the
supplementary USE corpus.

**Date:** 2026-09-17. **Lane:** C.

**How to see it.** The default corpus does not show this at all:

    python gui/font/tools/harfbuzz_sweep.py                              # misplaced 1
    python gui/font/tools/harfbuzz_sweep.py --corpus gui/font/tools/use-corpus.txt

The second reports `agree` 32203, `differ` 0, `misplaced` **40**, and every
one is the same shape — e.g. Javanese `U+A98F U+A9C0` on Hack-Bold, where we
put the mark at 1233 and HarfBuzz puts it at 0, which is the base's origin.
Tibetan `U+0F40 U+0F72 U+0F74` differs vertically instead: `(0, 0)` against
`(0, -1934)`.

**Not caused by the kern split.** Checked by restoring the previous
`scaled.rs` and re-running: 32203 / 40 either way.

**Where the cause is, and where it is not.** Not in fallback *placement*:
`fallback::positions_marks` declines these scripts deliberately, HarfBuzz's
Thai, Myanmar and USE shapers all set `fallback_position = false`, and
`TD-FONT-FALLBACK-CLASSES-SCRIPTS-IT-NEVER-PLACES` covers that. The
difference is the *other* half, which this crate already knows is a separate
question — see `fallback::tests::declining_to_place_a_mark_is_not_declining_
to_zero_it`. HarfBuzz's `zero_mark_widths_by_gdef` adjusts the offset by
`-advance` as it zeroes, parking the mark on its base; our equivalent shift in
`scaled.rs` (`let back = ...`) is gated on `synth_at`, which is
`!applies_gpos` — we placed it ourselves — and so does not fire here.

**Three hypotheses, all refuted, and the mechanism is now known.** HarfBuzz's
two GDEF zeroing modes differ in exactly this: `BY_GDEF_EARLY` calls
`zero_mark_widths_by_gdef(buffer, true)`, whose `true` means
`x_offset -= x_advance` *before* the advance is zeroed; `BY_GDEF_LATE` passes
`false` and does not move it. This crate already models the three modes, as
`fallback::Zeroing::{Never, BeforeGpos, AfterGpos}`, and already asks the
question, as `ScaledFont::zeroes_marks_first`. So the obvious fix is to gate
the back-shift on that instead of on `synth_at`. Measured, on both corpora:

| gate for the shift | default `misplaced` | USE `misplaced` |
|---|---|---|
| `synth_at` (*current*) | **1** | **40** |
| `zeroed_at` | -- | 65 |
| `early_at` (`zeroes_marks_first`) | 1315 | 105 |
| `synth_at \|\| early_at` | 31 | 65 |

So our `back` and HarfBuzz's `adjust_offsets` are **two different shifts**,
not one seen from two angles: replacing ours with theirs is catastrophic, and
adding theirs on top makes 30 runs worse that were right before. Whatever
`early_at` selects, it includes runs that must not be shifted.

**Then the run was printed, and the entry above was wrong about what
differs.** Javanese `U+A98F U+A9C0` on Hack-Bold, both glyphs `.notdef`:

    ours       advances [1233, 1233]   offsets [0, 0]
    harfbuzz   advances [1233, 0]      offsets [0, -1233]

**We do not zero the mark's advance at all here**, so this was never only
about the offset -- the run is twice as wide as HarfBuzz makes it. The offset
difference is the *consequence* of the zeroing that did not happen, since
HarfBuzz's shift is by the advance it is about to discard.

`marks[i]` is `zeroed_at[i] && if by_gdef { is_mark(gid) } else { glyph.mark }`.
The glyph is `.notdef`, which a face's `GDEF` does not classify as a mark, so
on any face that classifies its glyphs at all we ask about the glyph and get
`false`. HarfBuzz zeroes it regardless.

A fourth hypothesis, also refuted: `is_mark(gid) || glyph.mark`, so that the
character's category answers when `GDEF` declines. Default `misplaced` 1 ->
16, USE 40 -> 50. `GDEF` saying "not a mark" is evidently authoritative on a
face that classifies, and the difference is somewhere in *how HarfBuzz
decides mark-ness for a glyph its `GDEF` has no class for* -- which is not
the same question as "does this face classify".

**A fifth hypothesis, refuted, and it closes off a whole direction.** The
obvious reading of "how HarfBuzz decides mark-ness for a glyph its `GDEF` has
no class for" is a *per-glyph* fallback: consult the class, and where there is
none, use the character's general category. Implemented that way it changes
nothing at all, because `otl::glyph_class` returns `Some(0)` for any glyph
outside the table's ranges and the first attempt read that as an answer.
Correcting it to treat class 0 as "no class" -- which is what OpenType means
by it -- then gives default `misplaced` 1 -> 16 and USE 40 -> 50: *exactly*
the numbers the blunt `is_mark(gid) || glyph.mark` produced.

That is the useful part. Class 0 is not a small set; it is every glyph the
face did not explicitly list, so a category fallback over it is the same
thing as the OR, and a per-glyph rule of that shape cannot be the answer
however it is phrased. Whatever zeroes that mark in HarfBuzz is not a
mark-ness fallback at all.

Five hypotheses are now spent on the gate and the mark-ness test. The next
attempt should leave both alone and instrument the other end: print what our
zeroing pass *does* for this run -- `zeroed_at`, `marks`, the advance before
and after -- rather than guessing which predicate ought to select it.

**Why it is not urgent.** It arises only on a face with no glyphs for the
script, so every glyph in the run is already a box. The reason to fix it is
that the USE corpus is otherwise at `differ` 0 and would become a clean
instrument, the way the default corpus now is.

### MOSTLY FIXED 2026-09-17 — and the mechanism above was wrong

**In short:** the five refuted hypotheses were all aimed at the wrong pass.
HarfBuzz is not zeroing these marks in the routine this entry names; it is
doing it inside its *fallback mark positioning*, the very pass this entry
argued was not involved. Asking HarfBuzz directly settled in one run what
guessing had not in five. `misplaced` on the supplementary corpus is
**40 -> 15**, with the default corpus unmoved at 1 and `differ` still 0.

**How it was settled.** `uharfbuzz` exposes `hb_buffer_set_message_func`,
which reports each shaping stage as it happens. Printing the buffer's
advances and offsets at every stage for Javanese `U+A98F U+A9C0` on Hack-Bold
(`gui/font/tools/hb_trace.py`, added with this fix):

    start table GSUB script tag 'DFLT'   (unchanged)
    start fallback mark   [(0, 1233, 0, 0), (0, 1233, 0, 0)]
    end   fallback mark   [(0, 1233, 0, 0), (0, 0, -1233, 0)]

Both halves of the divergence appear in one stage, and it is not
`zero_mark_widths_by_gdef`. Two further measurements explain *why* that stage
runs at all, when this entry had reasoned it could not:

* `hb.ot_layout_has_glyph_classes(face)` is **0** for Hack-Bold. The face has
  no `GlyphClassDef`, so the entry's "on any face that classifies its glyphs
  at all" never applied to the face the divergence was measured on.
* The trace line above says `script tag 'DFLT'`. A face with no Javanese
  script table sends HarfBuzz to the **default** shaper, not USE — and the
  default shaper's `fallback_position` is `true`. The entry's "HarfBuzz's
  Thai, Myanmar and USE shapers all set `fallback_position = false`" is
  correct and irrelevant: none of them is the shaper that ran.

  This tree already models that, in `fallback::positions_marks`'s `simple`
  argument, fed from `face.shapes_as_default(script)`. It was working.

**The sixth hypothesis, which held.** The pass ran; the run had no marks in
it to place. Our role assignment asked `SubGlyph::mark`, which is `Mn` alone,
and `U+A9C0` JAVANESE PANGKON is `Mc`. HarfBuzz clusters a run for
`_hb_ot_shape_fallback_mark_position` with
`HB_UNICODE_GENERAL_CATEGORY_IS_MARK`, which counts `Mn`, `Mc` and `Me`. So
the mark was never in a cluster, nothing placed it and nothing zeroed it.

`SubGlyph` now carries **both** questions — `mark` for zeroing, `any_mark`
for clustering — and `mark_roles` asks the wide one. The narrow predicate
stays exactly where it was: a spacing combining mark occupies width, and
zeroing it would pile a Devanagari matra onto its consonant, which is what
`norm::is_mark`'s own doc warns about.

| corpus | before | after |
|---|---|---|
| default | `misplaced` 1 | `misplaced` **1** |
| supplementary USE | `misplaced` 40 | `misplaced` **15** |

**What the remaining 15 are.** Three strings on five faces each, and two
distinct shapes — neither of them the one fixed here:

* Tibetan `U+0F40 U+0F72 U+0F74` and `U+0F56 U+0F40 U+0FB2 U+0F0B U+0F64
  U+0F72 U+0F66`: the advance is now zeroed and agrees, and the *vertical*
  placement does not. We put the mark at `y` 0; HarfBuzz stacks it at
  -1934 and +1934, which are its combining classes 130 (above) and 132
  (below) resolved against the base's extents. So the next question is what
  our extents for a `.notdef` base come back as, since a zero-height base
  would place every mark at 0 exactly as observed.
* Sinhala `U+0D9A U+200D U+0DCA U+0DBB`: glyph 2 at 1233 against 0. A ZWJ
  sits between the letter and the virama, so this is likely about whether a
  default-ignorable breaks the cluster — `hide_ignorables` deliberately
  keeps a hidden mark's role, and the mirror question here is whether it
  keeps its place in the cluster the fallback walks.

**Lesson, and it is the same one as the fifth refutation.** Five hypotheses
were spent guessing which predicate HarfBuzz used, when HarfBuzz was
available the whole time to be asked which *pass* it used. The instrument
that settled it was fifteen lines of Python. Reach for the oracle's own
introspection before the next predicate.

### THEN 15 -> 5 — the Tibetan ten, and a guess of mine refuted in passing

**In short:** every Tibetan case above is fixed too. The cause was not the one
I guessed one paragraph earlier, and the guess is worth keeping visible: I
wrote that "a zero-height base would place every mark at 0 exactly as
observed". The base is not zero-height. `hb_font_get_glyph_extents` on
Hack-Bold's `.notdef` returns `y_bearing 1444, height -1806` — a real box.

**What it actually was.** `fallback::attach_class` had no arm for Tibetan's
combining classes, so 129, 130 and 132 fell through `other -> other` and
reached `place` as numbers it has no case for, leaving `y` at 0. Its doc said
they were left out because "nothing can ask the question", `tibt` being in
`COMPLEX_SCRIPTS` — the same false premise this entry started with, and
false for the same reason: `positions_marks` takes a `simple` argument, and a
`DFLT`-only face makes it true.

**The arithmetic confirms our formulas were right all along.** With the base
extents above and a gap of `upem/16` = 128, `place`'s own expressions give

    below:  1444 + (-1806 - 128) - 1444        = -1934
    above:  (1444 + 128) - (1444 + -1806)      = +1934

which are exactly HarfBuzz's two offsets. Nothing about the placement maths
needed changing; the classes simply never reached it.

**Why the signs are not inverted.** HarfBuzz puts `-1934` on the *first* mark,
and vowel `i` (U+0F72) is drawn above, which looks backwards until the
reordering is taken into account: `norm::display_class` permutes 130 to 132
and 132 to 131, so `u` sorts before `i` and the first mark in the buffer is
`u`, which does belong below. We already do that sort —
`sort_marks(&mut out, display_class)` — which is why adding the arms was
enough on its own.

| corpus | at the start | after clustering on `M*` | after the Tibetan arms |
|---|---|---|---|
| default | `misplaced` 1 | 1 | **1** |
| supplementary USE | `misplaced` 40 | 15 | **5** |

**All that is left is Sinhala** `U+0D9A U+200D U+0DCA U+0DBB` on five faces:
glyph 2 at 1233 against HarfBuzz's 0. A ZWJ sits between the letter and the
virama, so the question is whether a default-ignorable breaks the cluster the
fallback walks. `hide_ignorables` deliberately keeps a hidden mark's *role*;
the mirror question is whether it keeps its place in the cluster.

### FIXED 2026-09-17 — supplementary corpus `misplaced` 0

**In short:** it was the cluster, and the answer to the mirror question was
no. A blanked ZWJ kept `Role::Base`, which ended the run of marks before it
and started a new cluster, so the Sinhala virama measured itself against the
invisible joiner instead of the consonant — and an invisible glyph is
exactly as wide as nothing, so it landed a whole letter to the right.

**Both corpora are now clean instruments:**

| corpus | agree | differ | misplaced |
|---|---|---|---|
| default | 60490 | 0 | 1 |
| supplementary USE | 32243 | 0 | **0** |

The one remaining default-corpus case is the long-standing `SegUIVar` CGJ
entry, which is a deliberate divergence, not a defect.

**Why a third role rather than a special case.** `Role` had `Base` and
`Mark`, and a default ignorable is honestly neither. As a mark it would be
placed and zeroed, which is work on a glyph with nothing to draw; as a base
it cuts the cluster. `Role::Ignored` says what it is and the walk steps over
it, which is the same treatment `Role::Mark`'s own note already prescribed
for class-zero marks: *"Calling it a base instead restarts the measurement
halfway through a syllable."* The note was right and its reasoning simply had
not been carried across to the ignorables.

**Only a base is demoted.** A default ignorable that is *itself* a combining
mark — U+034F and the variation selectors are `Mn` — keeps `Role::Mark`,
which `hide_ignorables`' existing doc asks for and a test pins. It is
transparent to the walk either way.

**Note on the delete path.** A face with no space glyph deletes ignorables
outright, taking their roles with them, so it never had this bug. Only the
blanking path did, which is why it needed a face that *has* a space —
Hack-Bold — to show up at all.
