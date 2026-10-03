## TD-FONT-HAS-NO-JOINING-OR-REORDERING-SHAPER

**What.** Features are chosen by tag from a fixed list — `ccmp`, `locl`,
`liga`, `rlig`, `clig`, `calt`. The features every complex script actually
needs are not chosen that way: Arabic's `init`/`medi`/`fina`/`isol` are decided
by a joining-type state machine over the run, and Indic's
`rphf`/`half`/`pref`/`blwf`/`abvs`/`psts` by a per-cluster reordering pass. A
tag list cannot express either.

**Symptom, measured.** In the HarfBuzz sweep
(`gui/font/tools/harfbuzz_sweep.py`), 44 of 556 faces disagree on
`العربية` and 5 on `हिन्दी`. On `Amiri-Bold.ttf` we give
`[55, 84, 73, 65, 56, 90, 57]` — the isolated forms, every letter drawn as if
standing alone — where HarfBuzz gives the joined ones. Arabic rendered
unjoined is not "slightly off"; it is close to unreadable to someone who reads
Arabic. Devanagari is worse: ours is 6 glyphs to HarfBuzz's 4, because the
`i`-matra has not been reordered before its consonant and the conjunct has not
formed.

**Why it is filed rather than fixed.** It is a shaper per script family, not a
fix: HarfBuzz has separate `hb-ot-shaper-arabic`, `-indic`, `-khmer`,
`-myanmar` and `-use` modules for exactly this reason. Script selection was
the prerequisite — there is no point running an Arabic state machine over a
run until the run knows it is Arabic — and is now done.

**Proper fix.** Two shapers, in this order, since they cover the scripts a
desktop is most likely to meet: an Arabic joining pass (a joining-type table
plus the four positional features — mechanical and well-specified), then a
Universal Shaping Engine pass for Indic and South-East Asian, which is
substantially larger. Both hang off the run boundaries `script::runs` already
produces.

**Where.** `gui/font/src/gsub.rs` — the feature tag list in
`Substitutions::parse` and the module doc's "What is deliberately not
implemented"; `gui/font/src/scaled.rs` — `substitute_runs`, which is where a
per-script shaper would be dispatched.

**Resolved — the Arabic half (2026-08-14).** `gui/font/src/joining.rs` derives
each character's positional form from its `Joining_Type` and its nearest
non-transparent neighbours; `gui/font/src/otl.rs` records, per script, which
feature tags reached each lookup; `gui/font/src/gsub.rs` intersects that with
a per-glyph mask so a positional lookup reaches only the glyphs that take that
form. The tag list gained `isol`/`init`/`medi`/`fina` and `rclt`, and GSUB
type 3 (`AlternateSubst`) is now read, because Microsoft Uighur and others
write their positional forms as type 3. Measured on the same sweep: all 44
Arabic-capable faces now produce glyph-for-glyph identical output to HarfBuzz.
`Amiri-Bold.ttf` gives `[55, 1700, 1428, 1745, 3113, 2420, 1633]` where it
gave the isolated `[55, 84, 73, 65, 56, 90, 57]`. That is HarfBuzz's answer
exactly reversed, because HarfBuzz reverses an RTL buffer for the caller and
we do not reorder at all — see `TD-FONT-DOES-NOT-REORDER-RIGHT-TO-LEFT-TEXT`.
The design is recorded in `design-decisions.md` §412.

**Still open — the Indic half.** The 5 faces that disagree on `हिन्दी` are
untouched: the `i`-matra is still not reordered before its consonant and the
conjunct still does not form. That needs the Universal Shaping Engine pass,
which is the substantially larger of the two and is what this entry now
tracks.

**Resolved — the Indic half (2026-08-14).** Not the USE pass this entry
proposed, but HarfBuzz's own Indic shaper, which is what HarfBuzz actually runs
for the nine Indic scripts: `hb-ot-shaper-indic` is a separate module from
`hb-ot-shaper-use` precisely because the Indic scripts predate the universal
model and their reordering is specified against Uniscribe rather than against
USE's cluster grammar. Writing USE and pointing Devanagari at it would have
matched neither.

* `gui/font/src/indic.rs` — the character categories and positions, and
  `Syllable`, the cluster kinds.
* `gui/font/src/indic_machine.rs` — the syllable grammar, transcribed from
  HarfBuzz's Ragel machine.
* `gui/font/src/indic_shape.rs` — the shaper: `Plan` (what the face declares,
  probed once per run), the initial reordering (base finding, position
  assignment, the sort, the feature masks), the thirteen substitution stages,
  and the final reordering (matras, reph, pre-base forms).
* `gui/font/src/gsub.rs` — `apply_stages`, which runs a lookup set per stage
  and confines the shaper's own features to one syllable.

Measured on the sweep: `हिन्दी` went from 5 disagreeing faces to 0, and the
whole `misplaced` bucket to **0** across 556 faces × 23 strings, once the face —
not the character — was allowed to call the shaper off
(`TD-FONT-GATES-THE-MARK-FALLBACK-ON-THE-CHARACTERS-SCRIPT-NOT-THE-FONTS`).
The design is recorded in `design-decisions.md` §421, and why the shaper is
chosen by the *face* rather than by the character in §422.

**Still open — everything that is neither Arabic nor Indic.** Khmer, Myanmar,
Thai/Lao and the ~90 USE scripts still reach the default shaper: their
positional and reordering features are never asked for. No string in the sweep
corpus exercises them, so the cost is unmeasured rather than zero. USE is the
next shaper to write, and `indic_shape.rs`'s stage driver, syllable stamping
and `Plan` probing are the reusable parts of it. Filed on as
`TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE` — **all four of those shapers now
exist**, and that entry is closed in `known-issues-resolved.md` (`# Lane C`).
