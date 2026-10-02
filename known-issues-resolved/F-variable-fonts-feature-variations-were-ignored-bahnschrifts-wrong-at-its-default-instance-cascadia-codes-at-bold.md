### [F] Variable fonts' feature variations were ignored: Bahnschrift's `$ ¢ Ø ø` wrong at its default instance, Cascadia Code's `$` at bold -- 2026-09-26

**Status: FIXED 2026-09-26** (lane F), found and fixed in the same change.

**In short:** a variable font can swap glyphs for other designs at some of
its instances -- a dollar sign that needs a thinner stroke once the weight is
heavy -- through OpenType *feature variations* (the `rvrn` feature and the
`FeatureVariations` table). The shaper applied neither, so Bahnschrift drew
its `$ ¢ Ø ø ₵ ₡` in the wrong design even at its default instance, and
Cascadia Code and Cascadia Mono drew the regular `$` (and `฿`, `$>`, `<$`)
at weights from about 520 up.

**What was wrong, and the fix.** HarfBuzz enables `rvrn` for every run, in a
`GSUB` stage of its own before any other feature, and compiles its plan with
the feature tables of the first `FeatureVariationRecord` whose conditions
hold at the font's instance. Now so does this crate: `rvrn` is bit 0 of
`gsub::FEATURES` (in `ALWAYS`) with its own first stage in `apply_stages`,
and in `gpos::FEATURES`' default part; `otl::variations` reads the records
(condition format 1, as HarfBuzz 8.3 evaluates them); `Face` keeps a
`Substitutions` and a `Positioning` per record (`parse_varied`), and
`substitute_at`/`position_at`/`gpos_kerns` choose by the `ScaledFont`'s
coordinates.

**How to see it.** The HarfBuzz sweep's corpus now has `$ ¢ Ø ø`, which
Bahnschrift failed at its default instance; `harfbuzz_sweep.py --corpus ...
--axes wght=700` on Cascadia Code showed the rest (all agree now, at the
default, 300, 550, 600, 700 and `wdth=75`). Unit tests in `gsub.rs`: record
selection, condition semantics, and `rvrn` running before a lookup-order
earlier feature.
