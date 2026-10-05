### [F] Ideographs, and glyphs no script claims, are drawn unhinted -- 2026-09-26 -- **FIXED 2026-09-26**

**Status:** FIXED 2026-09-26 (lane F) — `gui/font/src/hint/cjk.rs` ports
`afcjk.c` and `afindic.c`, and `mod.rs` FreeType's dummy system (a glyph it
will not hint is still scaled from whole font units to 1/64 pixel, which is
what it does to a Latin style with no measurable zone). The fix was not the
one proposed below: FreeType's CJK system hints *both* axes in light mode
(moving a stem by at most 14/64 pixel), so the port does too, which
`glyph.rs`'s points and passes were made dimension-generic for. One
FreeType quirk is ported as it behaves: its CJK roundness pass never runs.
Checked with `hint_oracle.py`: every glyph of Malgun Gothic (10 sizes),
Microsoft YaHei, MS Gothic, SimSun, Yu Gothic, Microsoft JhengHei,
SimSun-ExtG and Noto Sans JP (CID-keyed CFF) agrees with FreeType, style,
points and both coordinates, as do the Latin fonts' fallback-style glyphs;
the fixture has CJK glyphs drawn for each rule (a wide stroke end, three
even stems, bars too close together, a ring, a fallback arrow).

**In short (as filed):** hinting (fitting text to the pixel grid) now works for nearly
every script, but not for Chinese, Japanese and Korean characters, nor for the
odd glyph in any font that no character reaches directly. Those are drawn
exactly as they were before hinting existed: correct, a little softer at small
sizes than the rest.

**Why.** The hinter is a port of FreeType's auto-hinter (design-decisions
§1325), and FreeType hints those glyphs with a second algorithm, its *CJK
writing system* (`src/autofit/afcjk.c`), which is not ported. FreeType files
every glyph no script claims under the same CJK style, so a Latin font's
`.notdef` and its unreachable components go that way too. Four scripts
FreeType puts in its Indic writing system (Limbu, Oriya, Syloti Nagri,
Tibetan) are in the same position: FreeType's Indic system is a stub that
borrows the CJK code.

**Where.** `gui/font/src/hint/mod.rs` (`System::Cjk`, `System::Indic`: no
metrics are made for them, so `Hinter::hint` returns `None`).

**The proper fix.** Port `afcjk.c`'s vertical, light-mode path the way
`latin.rs` ports `aflatin.c`: its blue zones (`af_cjk_metrics_init_blues`,
top and bottom of the ideographs in `AF_BLUE_STRING_CJK_TOP`/`_BOTTOM`, whose
`|` separates the "fill" letters from the others), its edge hinting
(`af_cjk_hint_edges`) and its own `align_edge_points`; the point passes in
`glyph.rs` are shared. Then extend `hint_oracle.py`'s check to a CJK font
(`C:\Windows\Fonts\msgothic.ttc`, Malgun Gothic) and the fallback style.

**How to see it.** `python gui/font/tools/hint_oracle.py <font>` reports these
glyphs under "unhinted here".
