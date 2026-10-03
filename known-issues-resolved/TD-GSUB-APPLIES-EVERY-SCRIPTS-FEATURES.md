## TD-GSUB-APPLIES-EVERY-SCRIPTS-FEATURES — ✅ FIXED 2026-08-14

**What.** The `GSUB`/`GPOS` walk in `gui/font/src/otl.rs` starts at the
FeatureList and takes *every* feature carrying a wanted tag, rather than
starting at the ScriptList and taking the features that the run's script and
language actually select. A face that registers the same feature tag under
several scripts therefore has all of those scripts' lookups applied to every
run, whatever the run is written in.

**Why it bites now.** This was a documented, mostly-theoretical limitation
while only `liga`/`rlig` were read: a ligature belonging to another script
almost never matches Latin glyphs, so the wrong lookups ran but did nothing.
Reading `ccmp` changes that. `ccmp` is precisely where a script puts its
normalisation rules, and those rules are meaningless — or wrong — outside it.

**Reproduce.** `cargo test -p osfont --target x86_64-pc-windows-gnu --test
host_fonts -- --ignored --nocapture installed_fonts_leave_plain_latin_alone`.
On a stock Windows host, `ebrima.ttf` and `ebrimabd.ttf` substitute the *space*
glyph in plain English prose: their `ccmp` lookup 15 is an extension-wrapped
type-1 format-2 subtable mapping glyph 3 (space) to 2220, and it belongs to one
of the African scripts Ebrima covers, not to Latin. Verified against an
independent Python parse of the table, so this is our *selection* being wrong,
not our *parsing*.

The damage is small — 2 faces of the 275 with `GSUB` on this host, and the
substituted glyph is a space variant — but it is a genuinely wrong glyph, and
the class of fault grows with every feature added.

**Proper fix.** Script and language selection, in two parts:

1. **The table walk.** Walk the ScriptList, pick the ScriptRecord for the run's
   script (falling back to `DFLT`), then its LangSys (falling back to the
   default), and intersect that LangSys's feature indices with the wanted tags.
   This is contained work in `otl.rs` and affects `kern.rs` and `mark.rs` too,
   since they share the walk.
2. **Script itemisation.** Deciding what a run's script *is* needs the Unicode
   Script property, which this crate does not have — a run must be split into
   same-script pieces before it can be shaped, which is also the prerequisite
   for bidi and for complex-script reordering. This is the larger half and is
   the reason (1) is not enough on its own.

Until both land, `installed_fonts_leave_plain_latin_alone` tolerates a small
proportion of faces changing plain Latin prose. When script selection works,
that count should drop from eight to the six Linux Libertine files, whose `Th`
ligature is correct.

**Where.** `gui/font/src/otl.rs` — `lookup_indices` (the FeatureList walk, and
the module doc's "What is not here"); `gui/font/src/gsub.rs` — the feature tag
list in `Substitutions::parse`; `gui/font/tests/host_fonts.rs` —
`installed_fonts_leave_plain_latin_alone`.

**Fixed 2026-08-14** (commit `6e0746636`), both parts, as designed above and
recorded in design-decisions.md §411.

1. `ByScript` in `otl.rs` walks the ScriptList, resolves every script the face
   registers once at parse time, and shares the decoded lookups keyed by
   LookupList index. `Substitutions::apply` takes the run's script and binary
   searches for it, falling back `dev2`→`deva`→`DFLT`→`dflt`.
2. `gui/font/src/script.rs` carries the Unicode Script property (generated
   into `script_tables.rs` from `fontTools.unicodedata`) and `script::runs`
   splits a piece list into maximal same-script stretches. The split happens
   in `ScaledFont::shape` *before* substitution, while glyphs are still one
   per piece — after anything ligates, a boundary counted in pieces is no
   longer a boundary counted in glyphs.

Ebrima no longer substitutes the space, and the same change fixed
`B-FONT-CALIBRI-SHAPES-A-FRACTION-SLASH-DIFFERENTLY-FROM-HARFBUZZ`, whose
cause turned out to be identical.

**The prediction in this entry was wrong, and the correction is the
interesting part.** It said the plain-Latin count "should drop from eight to
the six Linux Libertine files". It is *nine*, and all three non-Libertine
faces are correct: `segoesc`/`segoescb` have genuine Latin `calt`, and
`SansSerifCollection` maps `space` through its Latin `locl` — a feature this
crate had been skipping, and which was only safe to add once features were
script-scoped. All nine now agree with HarfBuzz glyph for glyph. The test's
bound is a proportion, not a list, which is why it kept working; a hard-coded
expected count would have had to be relaxed for a change that made the shaper
*more* correct.

**Successors.** Four narrower gaps remain and are filed separately:
`TD-FONT-IGNORES-LANGSYS-OVERRIDES`,
`TD-GPOS-APPLIES-EVERY-SCRIPTS-FEATURES`,
`TD-FONT-SCRIPT-RUNS-IGNORE-SCRIPT-EXTENSIONS` and
`TD-FONT-HAS-NO-JOINING-OR-REORDERING-SHAPER`.
