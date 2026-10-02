### [F] Superscripts, subscripts and small capitals are hinted with their script's ordinary zones -- 2026-09-26

**Status: FIXED 2026-09-26** (lane F) — the port has FreeType's feature styles
now, done the way the entry below proposed. `gen_autofit_tables.py` generates
all 87 styles (27 of them feature styles, each with its tag); `FaceHints::new`
lets each feature style claim, in FreeType's order, its feature's `GSUB`
output less its `GPOS` input (`Face::feature_style_glyphs`, after HarfBuzz's
`hb_ot_layout_collect_lookups` with a feature filter and feature variations,
`collect_glyphs`, and `would_substitute` with its coverage digest); and each
style measures its zones from reference letters shaped with its feature on,
through optional features the shaper now has (`gsub`/`gpos::OPTIONAL_FROM`,
`scaled::Extra`), off for every ordinary run. Every glyph of nine fonts is
sorted into FreeType's style for it and every hinted glyph agrees
(`hint_oracle.py`, which now compares the sorting too); the golden fixture
has small capitals and superscripts, one positioned, mutation-checked.

**In short (as filed):** a superscript ² or a small capital is aligned to the heights of
its script's ordinary letters (or, for ² and ₂, of the modifier letters), where
FreeType aligns it to heights measured from the font's own superscripts and
small capitals. The difference is a pixel here and there on those glyphs only;
everything else is unaffected.

**Why.** FreeType built with HarfBuzz -- as every desktop's is -- gives each
script *feature styles* besides its default one: small capitals (`smcp`,
`c2sc`), petite capitals (`pcap`, `c2cp`), superscripts (`sups`), subscripts
(`subs`), scientific inferiors (`sinf`), ordinals (`ordn`) and titling
(`titl`). Each claims the glyphs its OpenType feature produces, before the
default styles see them, and measures its zones from its reference letters
*with the feature applied*. The port has only the default styles.

**Where.** `gui/font/tools/gen_autofit_tables.py` (keeps only
`AF_COVERAGE_DEFAULT` styles), `gui/font/src/hint/mod.rs` (`FaceHints::new`'s
coverage).

**The proper fix.**
1. Generate the feature styles (the `META_STYLE_LATIN` expansions for Latin,
   Greek and Cyrillic) with their feature tags from `afcover.h`, in FreeType's
   order.
2. In coverage, before the default styles' `GSUB` pass: for each feature
   style, the output glyphs of its script's lookups for that feature
   (`Face::gsub_outputs` restricted to one feature tag), minus the glyphs its
   `GPOS` lookups for the feature take as input, and only if the feature
   substitutes at least one of the style's reference letters
   (`hb_ot_layout_lookup_would_substitute`).
3. In measuring the zones, shape each reference cluster with the feature on,
   and skip a cluster the feature leaves unchanged -- which needs the shaper
   to take an extra feature.

**How to see it.** `hint_oracle.py` on Noto Sans: the ~330 differing glyphs are
all small capitals and other feature forms (unmapped glyphs in `cyrl_dflt`)
and the super- and subscript digits (`latp_dflt`, `latb_dflt`).
