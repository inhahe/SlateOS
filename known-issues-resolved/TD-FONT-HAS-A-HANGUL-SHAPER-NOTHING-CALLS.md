## TD-FONT-HAS-A-HANGUL-SHAPER-NOTHING-CALLS — ✅ FIXED 2026-08-15

**What.** `gui/font/src/hangul.rs` is a complete, tested transcription of
HarfBuzz's `preprocess_text_hangul` — 673 lines, 19 tests, all passing — that is
**not declared in `lib.rs`** and therefore compiles nowhere and changes no
output. It was parked mid-task on an explicit halt, at the point where it worked
in isolation but was not yet connected.

**Why it is parked rather than either finished or deleted.** The connection is
all-or-nothing, and the half of it that was written first is a regression on its
own. Wiring the shaper means telling `norm::pieces` to stop normalizing Hangul —
HarfBuzz's Hangul shaper sets `HB_OT_SHAPE_NORMALIZATION_MODE_NONE` precisely
because composing first destroys the distinction the shaper reads. But `pieces`
composing `<L,V,T>` to a syllable is currently the *only* thing that makes
Korean render at all on the ordinary Korean text font, which ships the 11,172
precomposed syllables and no jamo. Exempt Hangul from normalization without the
shaper in place and that font draws three missing-glyph boxes where it used to
draw one correct syllable. So the `norm.rs` half was reverted and the module
kept: a tested, inert file loses nothing, whereas a half-wired one is worse than
neither.

**The four edits that connect it**, in the order they have to happen:

1. `norm.rs` — thread a private `enum Hangul { Normalize, LeaveAlone }` through
   `decompose_once`, `compose_pair`, `decompose_into` and `compose`; split `nfc`
   into `nfc` (which passes `Normalize`, because NFC is NFC and a question about
   *text* must get that answer) and a private `normalize(text, hangul)`. `pieces`
   then calls `normalize(text, Hangul::LeaveAlone)`, and `split_undrawable` calls
   `decompose_once(ch, Hangul::LeaveAlone)` — the latter because a syllable
   `hangul::preprocess` declined to split has been declined on grounds
   `split_undrawable` cannot see, namely that the face has no jamo either. Three
   call sites in `norm.rs`'s own tests need the new argument.
2. `gsub.rs` — add `b"ljmo"`, `b"vjmo"`, `b"tjmo"` to `FEATURES` with `LJMO`,
   `VJMO`, `TJMO` bit constants, and a `SubGlyph::jamo(gid, cluster,
   Option<Jamo>)` constructor that ORs the one jamo bit and **clears `CALT`**.
   Clearing `calt` is not an optimization: Noto Sans CJK and Source Han Sans file
   all of their jamo lookups under `calt`, and HarfBuzz's `setup_masks_hangul`
   turns it off for every L/V/T so those lookups cannot fire twice.
   `the_masks_match_the_feature_list` has to keep passing.
3. `scaled.rs::shape` — call `hangul::preprocess` immediately after
   `norm::pieces`, with `has_glyph = |ch| self.face.glyph_index(ch).is_some()`
   and `zero_width = ` has-glyph *and* zero horizontal advance; then choose
   between `SubGlyph::cursive` and `SubGlyph::jamo` in the piece loop on
   `hangul::is_jamo(ch)`. Guard the whole thing with `hangul::present` so a run
   with no Korean in it pays nothing.
4. `fallback.rs` — add `*b"hang"` to `NO_ZERO_WIDTH_MARKS` (the Hangul shaper's
   `zero_width_marks` is `NONE`) and **not** to `COMPLEX_SCRIPTS` (its
   `fallback_position` is `true`). Both lists are `is_sorted`-asserted.

**What it should buy.** 553 of the sweep's 892 remaining `differ` cases are the
single string `\u1100\u1161\u11a8` — jamo we compose to `각` and HarfBuzz leaves
as three glyphs. Expect `differ` 892 -> ~339. The residue after that is composed
Latin diacritics, which is a *different* and unsettled question: HarfBuzz
decomposes and recomposes against font coverage, which reverses the layering
`norm.rs`'s module doc deliberately chose (`nfc` pure Unicode, `fit_to_face` pure
font). That one is an operator question, not a bug.

**Where.** `gui/font/src/hangul.rs` (parked), `gui/font/src/lib.rs` (the missing
`mod hangul;`), and the three files named above. The reference is HarfBuzz's
`src/hb-ot-shaper-hangul.cc`.

**Resolution — 2026-08-15.** All four edits landed together with the missing
`mod hangul;`, and the prediction above held to the case. The HarfBuzz
differential sweep (556 host faces × 23 strings, 12,739 comparisons):

| bucket | before | after |
|---|---|---|
| `agree` | 11,847 | **12,400** |
| `differ` | 892 | **339** |
| `reordered` | 0 | 0 |
| `misplaced` | 0 | 0 |

`\u1100\u1161\u11a8` — all 553 of its cases — left the disagreement list
entirely, and nothing regressed into `reordered`/`misplaced`. `osfont` goes
from 482 to **501 passing tests**: the module's own 19 tests had never run
before, because a module that is not declared does not compile and therefore
does not test either. That is the sharper lesson here — "19 tests, all
passing" was a true statement about a file `cargo test` had never once
looked at.

Two notes on how the edits differ from the plan above. `gsub.rs`'s three new
feature tags are **appended** to `FEATURES` rather than inserted in tag order,
so that no existing bit constant shifts; the bits are `1 << 34/35/36`.
And `norm::nfc` lost its last production caller in the split, so it now
carries `#[cfg_attr(not(test), allow(dead_code, …))]` — it is kept deliberately
as the text-question half of the split (NFC is NFC), not as dead weight, and
the reason is written at its definition.

The residual 339 are exactly the composed-Latin-diacritics cases this entry
predicted (`\u1e09` 255, `\u212b` 57, `été` 10, …). They are **not** tracked
here as a bug; they are the layering question in `norm.rs`'s module doc, and
belong to the operator. See `open-questions.md`.
