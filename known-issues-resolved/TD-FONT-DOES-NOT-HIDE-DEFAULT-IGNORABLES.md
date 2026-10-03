## TD-FONT-DOES-NOT-HIDE-DEFAULT-IGNORABLES — RESOLVED 2026-08-15

**Resolved in two commits, because it was two bugs wearing one name.**

*Half one — erasing them* (`88ee69ca7`): `norm::ignorable` classifies the
character, `SubGlyph::ignorable` carries the answer and is cleared wherever a
`GSUB` lookup rewrites the glyph, and `ScaledFont::shape` replaces what is left
with the space glyph, or drops it where the face has none.

*Half two — stepping over them* (this commit): erasing an ignorable at the end
is not enough, because the lookups in between still saw it as a wall. `f ZWJ i`
did not ligate; a contextual alternate did not match across a soft hyphen. The
matcher now answers three ways rather than two, as HarfBuzz's does — hide,
*step over*, or consider — with the kind of ignorable and the kind of lookup
deciding which. See `design-decisions.md` §434 for the shape of that, and
`gui/font/src/skip.rs`'s `Joiners` for the table.

**Measured.** Host sweep, 556 faces × 60 strings: `differ` on `f\u200di` went
from 76 faces to 0, and `misplaced` from 331 to 170. Khmer probe: 45/45 before
and after, which is the point — the Indic-family features read the joiners
themselves and had to come through unchanged.

**The 170 that remain are a deliberate divergence, not a residue.** They are
every corpus string containing an ignorable, and in all of them the glyphs and
every *visible* glyph's position agree; what differs is the x of the erased,
zero-advance glyph itself. HarfBuzz spends a legacy `kern` on the right-hand
glyph's offset, so its erased glyph sits at the *unkerned* pen — 13 units
inside the following letter's image, for `a◌͏b` in Arial Rounded. We charge the
kern to the pair's left glyph, so ours sits exactly where the next glyph is
drawn. A caret asked to land on the ignorable's cluster wants ours. Recorded in
`design-decisions.md` §434; do not "fix" it without reading that first.

---

*The original entry follows, as filed.*

**What.** A handful of characters exist to instruct the shaper and are never
meant to be drawn: the zero-width joiner and non-joiner, the soft hyphen, the
bidi controls, the variation selectors, the byte-order mark. Once shaping is
over, HarfBuzz erases them — `hb_ot_hide_default_ignorables`, in
`hb_ot_substitute_post`, replaces each one's glyph with the face's `space`
glyph, or **deletes the glyph entirely** if the face has no space — and
`hb_ot_zero_width_default_ignorables`, during positioning, zeroes their
advances and x-offsets first. We do neither: `ScaledFont::shape` maps the
character through `cmap` like any other and returns whatever glyph came back.

**Symptom, measured.** The two strings the Khmer probe font disagrees on
(`gui/font/tools/khmer-corpus.txt`, the `\u17d2\u200d\u1781` and
`\u17d2\u200c\u1781` lines) are exactly this: HarfBuzz emits the space glyph
where we emit ZWJ's and ZWNJ's own glyphs. It is invisible in the host sweep
only because the built-in corpus has no string containing an ignorable that
the face also maps.

**Why it matters beyond the joiners.** This is crate-wide and
script-independent, and the joiner case is the *benign* one — a face that maps
ZWJ usually maps it to something blank anyway. The soft hyphen U+00AD is the
one that bites: fonts routinely map it to a real hyphen glyph, so a word
carrying a discretionary break renders with a hyphen sitting in the middle of
it whether or not the line broke there. The bidi controls and variation
selectors are the same shape of bug.

**One subtlety that is easy to get wrong.** HarfBuzz's predicate is
`(unicode_props() & UPROPS_MASK_IGNORABLE) && !_hb_glyph_info_substituted()` —
a character stops counting as ignorable the moment a GSUB lookup rewrites it,
because at that point the glyph is whatever the font asked for and is no
longer the control character. So the flag has to be *cleared on substitution*,
not merely tested at the end. And the set is HarfBuzz's own hard-coded list
(U+00AD, U+034F, U+061C, U+17B4–17B5, U+180B–180E, U+200B–200F, U+202A–202E,
U+2060–206F, U+FE00–FE0F, U+FEFF, U+FFF0–FFF8, U+1BCA0–1BCA3, U+1D173–1D17A,
U+E0000–E0FFF), *not* Unicode's `Default_Ignorable_Code_Point` property; using
the Unicode set would make the sweep disagree in the other direction.

**Proper fix.** A flag on `SubGlyph`, set in `scaled.rs`'s per-piece build loop
from the character, cleared at the three sites in `gsub.rs` that assign a
glyph id — `apply_single`, `apply_alternate`, the ligature path — and by
`apply_multiple`'s splice. Then in the loop that builds `out: Vec<ShapedGlyph>`
at the end of `shape`, zero the advance and offsets and substitute the space
glyph, or drop the glyph if the face maps no space. Corpus strings containing
a soft hyphen and the joiners go into `harfbuzz_sweep.py`'s built-in `CORPUS`
in the same change, so the fix is measured on all 556 host faces rather than
on the one probe font that happened to expose it.

**Where.** `gui/font/src/scaled.rs` — the per-piece loop that derives
`tab`/`klass`/`mark`/`indic` from each character, and the `out`-building loop
after it; `gui/font/src/gsub.rs` — `apply_single`, `apply_multiple`,
`apply_alternate` and the ligature path; `gui/font/tools/harfbuzz_sweep.py` —
`CORPUS`.
