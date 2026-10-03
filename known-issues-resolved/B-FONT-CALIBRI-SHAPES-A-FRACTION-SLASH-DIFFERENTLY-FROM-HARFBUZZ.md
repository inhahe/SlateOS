### B-FONT-CALIBRI-SHAPES-A-FRACTION-SLASH-DIFFERENTLY-FROM-HARFBUZZ. Three faces disagree by one glyph on `1/2` — 2026-08-14 — ✅ FIXED 2026-08-14 (`gui/font/src/otl.rs`, `gui/font/src/gsub.rs`)

**Where:** `gui/font/src/gsub.rs`, the substitution pass; the disagreement is
in whichever lookup rewrites `/` (or the digits around it) in Calibri's
`GSUB`. Not yet narrowed to a lookup type.

**Symptom.** Shaping the string `1/2` through `calibri.ttf`, `calibrib.ttf`
and `calibril.ttf` gives `[1005, 877, 1006]` where HarfBuzz gives
`[1005, 876, 1006]` — same glyph count, same outer glyphs, one differing
glyph in the middle. Glyph 876 vs 877 is almost certainly fraction slash
(U+2044) versus solidus, or two widths of the same mark.

**How it was found.** Cross-checking our shaper against HarfBuzz over all 556
installed faces and a 13-string corpus (see design-decisions.md §409):
6426 agreed, 324 differed, and after the subtable-budget fix and setting
aside HarfBuzz's own Unicode normalizer, these three faces are the entire
remaining disagreement.

**Reproduce.** Install `uharfbuzz` (`pip install uharfbuzz`), shape `"1/2"`
through `C:\Windows\Fonts\calibri.ttf` with features `kern`, `mark`, `mkmk`,
`curs` and `locl` disabled, and compare against `osfont`'s `substitute`.

**Why it is filed rather than fixed.** It is a single glyph in one family and
the correct answer is not obvious from the outside: it could be our
`calt`/`clig` picking a rule HarfBuzz's rule ordering rejects, or a lookup
type we do not implement (Alternate Substitution, type 3, is not implemented)
being skipped where HarfBuzz applies it. Deciding needs the actual lookup
dumped, not a guess.

**What the proper fix looks like.** Dump Calibri's `GSUB` lookups covering
glyph 876/877, find which lookup HarfBuzz applies that we do not (or which we
apply that it does not), and either implement the missing type or fix the
rule ordering. Then add the pair to
`installed_fonts_reach_lookups_past_the_subtable_budget`'s table of faces
with known answers.

**What it actually was.** Not a Calibri quirk and not a missing lookup type:
the shaper was applying another script's rules. Glyph 877 is produced by
`GSUB` lookup 92, which is reached only by `calt` feature 7 and `rclt` feature
153, both registered under **`arab`**. No `latn` feature reaches it. The
FeatureList-first walk matched on the tag `calt` alone and so ran Calibri's
Arabic contextual alternates over a Latin string, rewriting its slash.

**Fixed by** the script-selection change (design-decisions.md §411): the walk
now starts at the ScriptList, and a Latin run only sees features the `latn`
ScriptRecord selects. All three faces now shape `1/2` as `[1005, 876, 1006]`,
byte-identical to HarfBuzz, and the string has left the sweep report entirely.

**Lesson for the next one.** The entry above guessed at causes inside Calibri
(alternate substitution, rule ordering, fraction slash versus solidus) and all
of them were wrong, because the fault was not in the face. When an oracle
disagreement is concentrated in one *family*, ask which script the offending
lookup is filed under before asking what the lookup does.
