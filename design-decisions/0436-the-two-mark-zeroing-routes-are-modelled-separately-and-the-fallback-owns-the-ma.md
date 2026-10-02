## §436 — The two mark-zeroing routes are modelled separately, and the fallback owns the marks it places

**Date:** 2026-08-15
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** There are two entirely separate reasons a combining mark's width
gets set to zero, and we had them merged into one. One reason is "the font, or
failing that the character's Unicode category, says this glyph is a mark". The
other is "we could not find positioning instructions in the font, so we are
measuring the mark onto the letter ourselves, and a mark we place ourselves
must not also take up room". Merging them meant marks lost their width in cases
where they should have kept it, and in `ကို့` the dot below was drawn two
letters to the right of where it belongs.

**What HarfBuzz actually does, and what we did.**

*Route 1* — `zero_mark_widths_by_gdef`. Gated on a per-script flag
(`plan->zero_marks`, off for eleven scripts). Zeroes every glyph whose `GDEF`
class is mark — **or**, only when the face declares no `GDEF` classes at all,
every glyph whose Unicode general category is `Mn`. `hb_synthesize_glyph_classes`
runs behind `if (!hb_ot_layout_has_glyph_classes(...))`; it is an either/or.

*Route 2* — `_hb_ot_shape_fallback_mark_position`. Zeroes only the marks it
actually places (combining class ≠ 0), plus every `Mn` in the cluster when the
base glyph has no ink to measure against.

We had (a) the two `||`-ed together, so a face that classifies its glyphs had
the character's category second-guessing it, and (b) no per-script gate on
route 1 at all.

**The options.**

| | *What changes* |
|---|---|
| **Keep the union, patch the divergences as they surface** | Each newly-measured face needs another special case; the `simsun.ttc` shift stays until someone notices the next one. |
| **Model the two routes separately** (chosen) | A face that classifies its glyphs is believed; a face that does not falls back to categories; and the measuring pass zeroes only what it places. |

**The structural consequence, which is the part worth remembering.** Route 2
cannot be a single pass. HarfBuzz's `position_around_base` computes each mark's
horizontal offset as an accumulation over the advances *after* zeroing, so
`pens[base] - pens[i]` is only equal to it if the zeroing has already happened.
`synthesize_marks` is therefore **two phases**: walk the clusters and zero, then
compute the pens, then place. It is also where the class-zero bug lived — a mark
with combining class 0 is neither placed nor zeroed, so the old single-phase
splitter mistook it for a base and restarted the cluster on it.

**And "owning" the marks.** `SubGlyph::mark` is derived only for runs the
measuring fallback owns — not for runs `GPOS` reaches, and not for the complex
scripts whose shapers decline the fallback outright (Myanmar's does; its marks
are placed by `GPOS` or not at all). `Role::Base`/`Role::Mark(class)` carries
the answer, so a mark in a `GPOS`-owned run is deliberately `Role::Base` here.
That is not a lie about the character; it is the statement that this pass has no
business touching it.

**Known gap, believed unreachable.** Our cluster splitter reads `Mn` where
HarfBuzz reads `Mn|Mc|Me` — see `known-issues.md`. It needs a spacing matra
followed by a non-zero-class mark in one cluster, which canonical ordering does
not produce.

**Measured.** All twelve `shape_dump` probe cases byte-identical to HarfBuzz,
including `simsun.ttc` index 1 (`0;-128;0` → HarfBuzz's `0;-640;0`); Myanmar
sweep 58/58; full sweep back to its recorded `misplaced 170` baseline with
`agree` up by 19 and no new divergence anywhere else.

**If it is never revisited:** nothing degrades; this *is* the revisit.

**Where:** `gui/font/src/scaled.rs` (`Role`, `zeroed_at`, `places_marks`,
`zeroes_marks`, `synthesize_marks`, `hide_ignorables`),
`gui/font/src/fallback.rs` (`positions_marks`, `attach_class`, `place`),
`gui/font/src/norm.rs` (`is_mark`), `gui/font/src/gpos.rs` (`Run::marks`),
`known-issues.md` (`TD-FONT-HAS-NO-UNIVERSAL-SHAPING-ENGINE`).
