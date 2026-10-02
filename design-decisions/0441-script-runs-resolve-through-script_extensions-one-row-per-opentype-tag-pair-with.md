## §441 — Script runs resolve through `Script_Extensions`, one row per OpenType tag pair, with direction cut in afterwards

**Date:** 2026-08-16
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Some characters are shared between writing systems. The Arabic
zero `٠` is also how Thaana and Yezidi write zero; the Devanagari full stop `।`
is used by twenty-one Indian scripts. Text has to be cut into stretches of one
script before a font can be asked for that script's rules, and a shared
character sitting in the middle of a word must not cut it — ask a Thaana font
for Arabic rules and you get Arabic-specific substitutions applied to Thaana
letters. Unicode records the sharing as `Script_Extensions`, and this change
resolves runs through it. Three sub-decisions had a real case on both sides.

**One table row per OpenType tag pair, not per Unicode script.** A run
boundary is a change of *row index*, so two Unicode scripts that OpenType files
under one tag are one script here.

*The alternative* — a row per Unicode script, comparing tags to decide
sameness — is the more faithful model of Unicode and was rejected because it is
the wrong faithfulness. Hiragana and Katakana are both `kana`, and they are the
only pair that collides, so the entire practical effect of keeping them apart
is that ordinary Japanese gets cut at every change between the two — for a
difference the font cannot express, since it has exactly one `kana` feature
list to offer either way. Rows also make the extension sets smaller, because a
set naming both collapses to one member.

**A character with no `Script` of its own may narrow a run but never end
one.** When the intersection empties, a character that *has* a script (the
Arabic zero, whose `Script` is Arabic) ends the run and starts a new one; one
that does not (a danda, a tatweel, any combining mark) is left in the run
unchanged.

*The alternative* — treat an empty intersection as a boundary regardless — is
what UAX #24's plain statement suggests and is what was implemented first. It
failed a test written from first principles: `"א" + U+0301 COMBINING ACUTE`
came out as two runs, because U+0301's extension set names eight scripts and
Hebrew is not among them. A mark in a run of its own is a mark whose base's
`GPOS` mark-attachment can never reach it, so the accent lands at the origin
instead of over the letter. The rule that fixes it is stateable in one line —
such a character has an *affinity*, not an identity, and an affinity may refine
a decision but must not make one — and it costs nothing, because a character
with no script had no claim to press in the first place.

**Script is resolved over the whole text first; direction boundaries are cut
into the answer afterwards.** Two passes (`by_script`, then `runs`) rather than
one.

*The alternative* — one pass, closing the open script when the direction turns
— is simpler and is what this module did before. It is also measurably wrong.
In `"ހ٠ހ"` (Thaana with an Arabic-Indic digit in it) the digit is bidi class
`AN`, and rule I2 raises it to an even level inside odd-level Thaana, so it is
its own directional run. A one-pass splitter re-derives the script inside that
run, sees one Arabic-Indic digit, and calls it Arabic — which showed up in the
HarfBuzz sweep as a real disagreement on `SansSerifCollection.ttf`, a face that
registers a `locl` under `arab` and none under Thaana. Pango
(`PangoScriptIter`) and Blink (`RunSegmenter`) both resolve script over the
whole text and intersect with bidi afterwards, for this reason.

*The exception to it*, which the sweep also found: a character with **no**
script at all belongs to the directional run it is *drawn* in. The space in
`"hello שלום world"` is scriptless, so under a whole-text pass it extends the
Hebrew run — and the direction cut then strands it in a one-glyph Hebrew run,
losing the kern between it and `w`. It joins what follows it instead. Only
scriptless characters move this way; one that merely shares its scripts with
its neighbours keeps them across the turn, which is what preserves the Thaana
answer above.

**How it is measured.** 556 faces × 95 strings against HarfBuzz: `agree`
51422 → 51423, `differ` 1179 → 1178, `misplaced` 170 and `reordered` 0
unchanged. The corpus gained a section of shared-character strings — the three
cases above plus `あーア` (which must stay one `kana` run) — so a regression
here is caught by the oracle and not only by the unit tests.

**Where it lives.** `gui/font/src/script.rs` — `ScriptSet`, `by_script`,
`runs`, and the module doc; `gui/font/tools/gen_script_tables.py` —
`extension_ranges`, `pool`, `row_for`; `gui/font/src/script_tables.rs` —
generated, `SCRIPT_EXT_RANGES` / `SCRIPT_EXT_POOL` / `WIDEST_EXTENSION`;
`gui/font/tools/harfbuzz_sweep.py` — the corpus section.
