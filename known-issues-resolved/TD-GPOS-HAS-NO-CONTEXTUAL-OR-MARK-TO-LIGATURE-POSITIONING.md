## TD-GPOS-HAS-NO-CONTEXTUAL-OR-MARK-TO-LIGATURE-POSITIONING

**Status:** FIXED 2026-08-16 -- the resolution note in the entry below gives the change; on `main` since. Stamped 2026-09-28 by lane F, `gui/font`'s owner since the six-lane split, so that the heading no longer reads as unfixed. Moved to `known-issues-resolved/` on 2026-10-05 (lane F).

**What.** `gui/font/src/gpos.rs` dispatches `GPOS` lookup types 1, 2, 3, 4 and
6. Three types are parsed past and ignored: 5 (mark-to-ligature), 7
(contextual positioning) and 8 (chained contextual positioning). Device tables
— the per-ppem correction a `ValueRecord` can point at with the
`X_PLACEMENT_DEVICE`/`Y_PLACEMENT_DEVICE`/`X_ADVANCE_DEVICE`/`Y_ADVANCE_DEVICE`
formats — are skipped over for their size but never read.

**Symptom, measured.** Scheherazade-Regular is the one face still disagreeing
with HarfBuzz on `العربية` after the unified pass landed: our glyph 3 sits at
x=1280, HarfBuzz's at x=1150. It reaches the adjustment through a type-8
chained rule, so we never apply it. Type 5 shows up wherever a script both
ligates and takes marks — an Arabic lam-alef with a vowel sign on it, or a
Devanagari conjunct — because the mark has to attach to a numbered *component*
of the ligature glyph rather than to the glyph as a whole; without it the mark
lands on the ligature's single origin. Device tables only bite at small ppem
on faces that ship hinted corrections, and are the smallest of the three.

**Why it is filed rather than fixed.** Types 7 and 8 are byte-for-byte the
same three subtable formats as `GSUB`'s types 5 and 6, which
`gui/font/src/gsub.rs` already reads (`context_match`, `chain_match`,
`chain_rule`) — but their *action* differs. A `GSUB` contextual rule
substitutes at a matched position; a `GPOS` one runs a nested *positioning*
lookup there. So the matching is shareable and the record application is not,
and the recursion has to re-enter `gpos.rs`'s per-lookup apply carrying both
the skipper and a depth budget. That is a refactor of two modules' entry
points, not an addition to a dispatch table.

**Proper fix.** Lift `gsub.rs`'s rule matching out into a module both layout
tables use, generic over what happens at a matched position, and give
`gpos.rs` an apply-one-lookup-by-index entry point the recursion can call
with the same `MAX_NESTING` budget `gsub.rs` already defines. Type 5 is
independent and much smaller — it is type 4's reader with a component index
selecting which anchor array to read — so it can land first. Device tables
are a `ValueRecord` reader change plus a ppem the pass is not currently told.

**Where.** `gui/font/src/gpos.rs` — `Adjust::apply` and its dispatch;
`gui/font/src/gsub.rs` — `context_match` / `chain_match` / `chain_rule`, the
matchers to be shared; `gui/font/src/mark.rs` — the anchor reader type 5
would reuse.

**Type 5 fixed** (2026-08-14). Mark-to-ligature attachment now works; types 7
and 8 and device tables remain open, so this entry stays.

The estimate above — "type 4's reader with a component index" — was wrong, and
wrong in the way that mattered: *nothing in our glyph run recorded which
component a mark belonged to*, so there was no index to pass. HarfBuzz carries
it in a per-glyph `lig_props` byte written during ligature substitution, and
the honest fix was to port that bookkeeping rather than to fall back on
HarfBuzz's degenerate "use the last component" path for every mark.

So `gui/font/src/gsub.rs` gained a `Lig` on every `SubGlyph` — HarfBuzz's
`lig_props` with the bit-packing undone — and three pieces of `ligate_input`
transcribed onto it:

* `stamp_components` writes it. It keeps HarfBuzz's three cases: a base joining
  with nothing but marks stays a *base* (no id) so further marks can still
  attach to it; a ligature of nothing but marks keeps its existing id so it
  still belongs to the ligature its components were on; anything else gets a
  fresh id and every glyph the flag skipped over is numbered with the component
  it followed. The walk continues past the last component for as long as the
  glyphs still belong to it, because a component may itself be a ligature whose
  marks stand after the whole match.
* `ligation_allowed` enforces HarfBuzz's `match_input` legality rules, so a
  mark already inside one ligature cannot be pulled out and joined to a
  stranger — with the `LIGBASE_MAY_SKIP` exception for when the earlier
  ligature's base is a glyph this lookup's flag hides.
* `apply_multiple` numbers the pieces of a decomposition, and
  `Lig::components_in_ligation` makes every piece after the first worth *zero*
  components to a later ligature — matching how mark-to-base attaches a mark
  only to the first piece.

`gui/font/src/mark.rs` was split so the mark side of types 4, 5 and 6 is read
once (`marked`), with `attachment` and the new `lig_attachment` each supplying
their own way of finding the anchor to attach to. The one place type 5 differs
from type 4 in more than naming: a `LigatureArray` holds *offsets* to
per-ligature tables rather than one grid (ligatures differ in component count),
and the anchor offsets inside those are taken from the `LigatureAttach`, not
from the array.

**Measured.** The sweep corpus had no string that could show the bug, so one
was added: `\u0644\u064e\u0627` (LAM, FATHA, ALEF — the lam and alef ligate
across the fatha, which must then land over the *first* component). Over 556
faces, 56 of which ship a type-5 lookup: with the dispatch entry removed,
agree 10055 / misplaced 125; with it, **agree 10081 / misplaced 99** — 26
face×string cases fixed and none broken.

**Types 7 and 8 fixed** (2026-08-14). Only device tables remain open, so this
entry still stays.

The plan above held. `gui/font/src/context.rs` now holds the matching both
tables share — `context_match`, `chain_match` and everything under them, moved
out of `gsub.rs` unchanged — and `gpos.rs` grew an `at` entry point the
recursion re-enters through with the same `MAX_NESTING` budget. The asymmetry
the entry predicted is real and is the whole difference between the two
`nested` functions: `gsub::apply_nested` has to track where each matched glyph
moved to and mark the ones a ligature swallowed as absent, because a
substitution can grow or shrink the run under a later record's feet;
`gpos::nested` runs each record against the position the match reported,
because positioning never changes the run's length. `Positioning` also had to
start keeping the LookupList offset: a contextual rule names its lookups by
index into that list, and they may be lookups no feature reaches — which is
how a face hides a helper.

One thing that was *not* predicted: recursion must not re-test the skipper.
HarfBuzz's `apply_lookup` calls `dispatch` directly and lets the invoked
subtable's own coverage decide; a nested lookup is named by index, not found
by scanning, so the feature mask and the ignore flags have already had their
say. `gsub::apply_at` had quietly worked this way already and `gpos::at`
matches it.

**Measured.** Finding a string that reaches these lookups took two tries. The
first — pointed Hebrew — reaches none: `DavidCLM-Medium.otf`'s 24 type-8
subtables all require a *meteg* (`U+05BD`, glyph 114) in the lookahead, and
the corpus had none, so 274k subtable attempts across the host font set
matched zero times and the dispatch looked dead when it was merely unreached.
The second, `\u05dc\u05dc\u05b8\u05bd` (LAMED, LAMED QAMATS METEG), is the
case the rules exist for: a meteg shares the space under a letter with
whatever vowel is already there, so the face shifts the vowel aside with a
chained rule keyed on "letter, vowel, meteg". Over the 18 Hebrew faces on this
host: with the two types in `KINDS`, **18/18 agree with HarfBuzz**; with them
removed, **18/18 misplaced**. On `DavidCLM-Medium.otf` specifically the meteg
lands at x=77 and the qamats at x=315, both exactly HarfBuzz's, against x=2 and
x=240 without.

**Status: FIXED** (2026-08-16). Device tables were the last open item, so with
them read this entry is closed and moves to `known-issues-resolved.md` once it
has been on `main` through a boot test.

`gui/font/src/device.rs` is the reader; `Ppem` — a pixel size and the em it is
measured against — is threaded from `ScaledFont` through `Face::ppem` onto
`Run`, and from there into `Value::read`, `mark::anchor` and `Kerning::pair`.
All four value-record fields and both anchor axes are corrected, across single,
pair, cursive, mark-to-base, mark-to-ligature and mark-to-mark.

The entry's own plan for this item — "a `ValueRecord` reader change plus a ppem
the pass is not currently told" — named the wrong half. `gui/font/tools/
device_survey.py` was written to check it and reports that on this host **not
one value record carries a real device table**: all 152 hang off *anchors*, in
five faces (`micross.ttf` 130, `mmrtext.ttf` 4, `mmrtextb.ttf` 4, `taile.ttf`
7, `taileb.ttf` 7). The half the entry named is the half that never fires here;
the half it omitted is where all the effect is. Both were implemented.

Two things the survey settled that were not going to be settled by reading the
spec:

* **The format word must be read before the size range**, not after. 9,215 of
  this host's device-table slots are `VariationIndex` records (`deltaFormat`
  `0x8000`), which reuse the first four bytes as indices into a variable font's
  `ItemVariationStore`. Read as a `startSize`/`endSize` pair, those indices
  *bracket* an ordinary UI size often enough to matter: 3,146 of them at 9
  ppem, 3,086 at 12, 2,832 at 16. A range-check-first reader does not return a
  slightly wrong correction for those, it returns an arbitrary one.
* **The size a cross-check runs at has to be read off the fonts.** All 130 of
  `micross.ttf`'s tables name `startSize == endSize == 11`. A sweep at 12 ppem
  reaches none of them and reports agreement that both halves obtained by doing
  nothing — which is exactly what the first run of it did.

**Measured.** `harfbuzz_sweep.py` grew `--ppem N`, which sets `ppem` on the
HarfBuzz font while leaving its scale at the em (HarfBuzz gates device tables
on `font->x_ppem` and computes `pixels * x_scale / ppem`, so both halves then
report design units with the correction folded in) and opens our face at N
pixels, dividing the positions back. On `micross.ttf` shaping LAM-FATHA-ALEF
the fatha's y goes 380 → **8** at 11 ppem and back to 380 at 10 and 12; on
`mmrtext.ttf` shaping NNYA + MEDIAL HA it goes 650 → **735** at 24 ppem.
HarfBuzz answers 8 and 735 at those sizes and 380 and 650 either side of them —
identical on every glyph, truncation included. Across all 556 host faces the
full sweep at 11 ppem is **byte-for-byte the sizeless one** (48,087 agree, 170
misplaced, 1,178 differ, 0 reordered); at 24 ppem with the new Myanmar corpus
string it is 48,643 agree — the same numbers plus one string agreeing on all
556 faces. `cargo test -p osfont` is 701 passing, up from 686.

Twelve of the 152 tables (Tai Le's) are not reachable from any two- or
three-character sequence in the script's block — searched exhaustively against
HarfBuzz at every size in their ranges — so they are covered by the unit tests
in `device.rs` and by nothing else.
