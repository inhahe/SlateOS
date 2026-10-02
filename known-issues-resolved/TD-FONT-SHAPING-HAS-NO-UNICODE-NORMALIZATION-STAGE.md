### TD-FONT-SHAPING-HAS-NO-UNICODE-NORMALIZATION-STAGE. `e` + combining acute is not composed before GSUB — 2026-08-14 — FIXED 2026-08-14

**Where:** `gui/font/src/shape.rs`, between mapping characters to glyphs and
running `gsub::Substitutions::apply`. There is no normalization stage at all.

**Symptom.** Shaping `"e\u{301}"` gives two glyphs — the base letter and a
combining accent placed by GPOS — where HarfBuzz gives the single
precomposed glyph for `é`. This affects **288 of the 556 installed faces**,
which is by far the largest single class of disagreement with HarfBuzz.

**Why it is not a GSUB bug.** HarfBuzz runs its own Unicode normalizer
(`hb-ot-shape-normalize`) as a distinct stage before applying GSUB. Real
shapers compose to NFC and then, if the font cannot render the composed
form, decompose again and place marks. We do neither: we take the string's
code points as given.

**Consequence today.** The output is not *wrong* — the accent is attached at
its GPOS anchor and looks right — but it is a different glyph run from every
other shaper, which means metrics, hit-testing offsets and any future
comparison against a reference rendering will disagree. It also means a face
that has a good precomposed `é` glyph but poor anchors renders worse than it
should.

**What the proper fix looks like.** A normalization stage in `shape.rs` that
runs before substitution: compose to NFC using a compact Unicode composition
table (the canonical-composition pairs, not the whole UCD), check the
composed character has a glyph in the face, and fall back to the decomposed
form when it does not. The decompose-when-unmappable direction matters as
much as the compose direction — that is how a face without `é` still shows
an accented e.

**Fixed** in `4aa237205` by adding `gui/font/src/norm.rs`, a normalization
stage that runs in `ScaledFont::shape` before any `cmap` lookup. It is two
layers: `nfc()` is pure Unicode and never sees a font, and `fit_to_face()`
then takes a character back apart when the face cannot draw it. Tables are
generated from the UCD by `gui/font/tools/gen_norm_tables.py`.

Splitting is decided by the base — a character comes apart only if the face
can draw what the decomposition chain bottoms out at, and marks ride along
either way. Both halves of that rule were measured against HarfBuzz over
every installed face; see design-decisions.md §410 for why, and for the three
classes of remaining disagreement that are deliberate rather than defects.

After the fix the corpus sweep over all 556 faces agrees on 9368 of 10008
runs, and the 288-face class this entry describes is gone.
