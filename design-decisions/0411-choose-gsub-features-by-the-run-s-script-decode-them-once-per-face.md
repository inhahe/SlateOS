## §411 — Choose `GSUB` features by the run's script; decode them once per face

**Date:** 2026-08-14
**Lane:** C
**Decided by:** Claude (autonomous)

An OpenType feature tag is not unique. A face that supports both Arabic and
Latin registers a `liga` for each, filed under different ScriptRecords, and
they mean entirely different things. The walk in `otl.rs` used to start at the
FeatureList and take every feature carrying a wanted tag, so both applied to
every run. That is not a theoretical hazard — it was two live bugs on a stock
Windows host, and the second one had been mis-filed as a font quirk:

* `calibri.ttf` shaped `1/2` as `[1005, 877, 1006]` where every other shaper
  gives `[1005, 876, 1006]`. Lookup 92 is reached only by `calt` and `rclt`
  registered under `arab`; it rewrote the slash of a Latin string.
  (`B-FONT-CALIBRI-SHAPES-A-FRACTION-SLASH-DIFFERENTLY-FROM-HARFBUZZ`.)
* `ebrima.ttf` substituted the *space* glyph in plain English prose from a
  `ccmp` belonging to one of the African scripts it covers.
  (`TD-GSUB-APPLIES-EVERY-SCRIPTS-FEATURES`.)

### Selection is per run; decoding is per face

The obvious implementation walks the ScriptList at shaping time. It was
measured and rejected: the worst face installed here re-reads 1874 subtable
offsets to answer one string, and `ScaledFont::shape` is on a
per-label-per-frame path. The obvious alternative — decode once per script and
keep a table per script — duplicates every lookup a face shares between
scripts, which on a pan-Unicode face is nearly all of them.

`ByScript` does neither. It resolves every script the face registers up front,
takes the *union* of their lookup indices, sorts and dedups it, and decodes
that list once. Each script then keeps only positions into the shared vector.
Selecting at shaping time is a binary search on a 4-byte tag.

Sorting the union is not incidental. Lookups must be applied in LookupList
order — that ordering is the whole mechanism by which `ccmp` runs before the
ligatures that depend on it — and decoding in sorted index order is what makes
each script's stored positions ascending, which is what makes "iterate the
positions in order" the correct application order rather than a coincidence.

**Pro:** one decode per face, no duplication, O(log n) selection, and the
ordering invariant falls out of the data layout instead of being asserted.
**Con:** a face's lookups are decoded even for scripts the user never types,
so the parse cost is the union rather than the subset. That cost is paid once
at face load and is bounded by `MAX_SUBTABLES`; the shaping path — the one
that runs per label per frame — pays nothing for it.

The subtable budget (`MAX_SUBTABLES`) is shared across the union rather than
per script. A per-script budget was tried first and is wrong twice over: it
multiplies the worst-case work by the script count, which is exactly what the
budget exists to bound, and it makes the answer for a Latin run depend on how
many other writing systems the face happens to cover.

### The fallback chain belongs to the run, not to the font

A run asks for its script, then that script's older OpenType spelling
(`dev2` → `deva`), then `DFLT`, then `dflt`. `DFLT` is the registered tag;
`dflt` is a misspelling real fonts ship and every shaper accepts. Text with no
script of its own — `"123 456"` — starts at `DFLT`, which is what HarfBuzz does
with a `Common` buffer.

The chain is applied when a *run* looks a script up. It is deliberately not
applied when `ByScript::parse` indexes the font, where each script is filed
under its own tag exactly: doing it there would file `DFLT`'s lookups under
every script's name and make the fallback unobservable.

### `GSUB` selects; `GPOS` does not

The asymmetry is deliberate. `GSUB` rewrites glyph *identity*, so a
wrong-script rule corrupts text. `GPOS` only moves glyphs, and every
positioning subtable is gated on glyph coverage, so a face's Arabic `kern`
simply does not cover Latin glyphs. `GPOS` is also reached through
`ScaledFont::kern(left, right)` — a public API that is handed a glyph pair with
no run behind it and therefore has no script to select with. It keeps the
union over all scripts through `feature_subtables`, tracked as
`TD-GPOS-APPLIES-EVERY-SCRIPTS-FEATURES`.

**Pro:** fixes the whole class of bug where it can corrupt text, without
inventing a script for a two-glyph positioning query.
**Con:** the two halves of the same table walk now answer differently, which is
a thing a reader has to be told rather than infer. Hence this section and the
note in `otl.rs`'s module doc.

### Reading `locl` became safe, and was overdue

`locl` is on by default in every shaper. This crate skipped it on the grounds
that it is language-specific and applying it blind would hand a reader some
other locale's letterforms. Under the old script-blind walk that was right. It
is no longer: only DefaultLangSys is read, so what `locl` yields is the face's
*default* localization — precisely what a shaper applies when the caller names
no language.

Skipping it was visible. `SansSerifCollection.ttf` maps `space` through its
Latin `locl`, so every space in every Latin string came out as the wrong glyph
on that face. Adding the tag fixed four of the five strings it disagreed with
HarfBuzz about.

### What the oracle says now

556 faces x 18 strings: 9066 agree, 942 differ, and every differing string is
accounted for:

| Count | String | Why |
|---|---|---|
| 553 | `각` | §410 class 1, jamo composition |
| 255 | `ḉ` | §410 class 3, HarfBuzz's own spelling-dependence |
| 57 | `Å` | §410 class 2, singleton folding |
| 44 | `العربية` | no Arabic joining shaper yet — fixed in §412, where these 44 move to `reversed` (identical glyphs, RTL order) |
| 27 | `été`, `e◌́te◌́`, `c◌̧◌́` | §410 class 3 |
| 5 | `हिन्दी` | no Indic reordering shaper yet |
| 1 | `hello שלום world` | we itemize the string, HarfBuzz guesses one script for it |

The last row is the change working. HarfBuzz's `guess_segment_properties`
assigns one script to the whole buffer, so it applies Latin's `locl` to the
space between the Hebrew and the English. We give that space to the Hebrew run,
which is what UAX #24 says and what any real itemizer does.

**Code:** `gui/font/src/script.rs`, `gui/font/src/script_tables.rs`
(generated by `gui/font/tools/gen_script_tables.py`), `ByScript` in
`gui/font/src/otl.rs`, `Substitutions` in `gui/font/src/gsub.rs`,
`ScaledFont::substitute_runs` in `gui/font/src/scaled.rs`. Checked by
`gui/font/examples/shape_dump.rs` + `gui/font/tools/harfbuzz_sweep.py`.
