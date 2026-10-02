## 1325. The `hinting` font setting runs a port of FreeType's auto-hinter, light mode, checked against FreeType point for point

**Date:** 2026-09-26
**Lane:** F
**Decided by:** Claude (autonomous) -- within §86c, which left "a
vertical-only autohinter" as the deferred-not-rejected alternative to a
TrueType bytecode interpreter.

**In short:** with hinting on (the setting's default), small text is now
fitted to the pixel grid the way Linux desktops fit it: every glyph's
horizontal lines -- baseline, x-height, cap height, the bars of an `e` or an
`H` -- are moved up or down so they land cleanly on pixel rows, and no
stroke changes thickness. Letters move only up and down; Chinese, Japanese
and Korean characters, and symbols no script claims, move a little sideways
too, as FreeType moves them. The code is a translation of FreeType's own
auto-hinter rather than a new design, so it makes the same choices FreeType
does, and a tool compares the two glyph by glyph.

**Decision.**

* **A port, not a reimplementation.** `gui/font/src/hint/` is FreeType
  2.13.2's `autofit` module translated function by function: `glyph.rs` from
  `afhints.c`, `latin.rs` from `aflatin.c`, its fixed-point arithmetic kept
  to the bit (`fixed.rs`), its script tables generated from its sources
  (`tools/gen_autofit_tables.py`). A first attempt was an original
  hinter in FreeType's spirit; it matched FreeType's glyph *bounds* but not
  its interiors -- it rounded stems to whole pixels, which light mode never
  does -- and there was no way to tell which of its hundreds of small choices
  were wrong. Hinting is a chain of rounding decisions: the only way to get
  FreeType's look is to make FreeType's decisions.
* **Light mode only.** Stems keep their designed width, and thin ones are
  centred on a pixel row (fontconfig's `hintslight`, most Linux desktops'
  default). For the Latin writing system that means the vertical axis only;
  FreeType's CJK system hints both axes in light mode too, moving a stem by
  at most 14/64 pixel, and so does the port. Advances and kerning are the
  unhinted ones either way. The same for grey, LCD and smoothing-off
  rendering.
* **Every script FreeType's Latin writing system serves** -- most of them:
  Latin, Greek, Cyrillic, Arabic, Hebrew, Armenian, the Brahmic scripts, Thai
  and some fifty more, each measured from its own reference letters. Each
  glyph is given a style as FreeType gives it one (by `cmap`, then by the
  `GSUB` lookups that produce it), and the reference letters are shaped by
  this crate's shaper, as FreeType has HarfBuzz shape them. That includes
  FreeType's *feature styles* for Latin, Greek and Cyrillic -- small and
  petite capitals, ordinals, superscripts, subscripts, scientific inferiors,
  titling forms -- which claim what their OpenType feature substitutes in
  (less what it also positions) and measure their zones from reference
  letters shaped with the feature on: the shaper has optional features for
  that, off for every ordinary run.
* **And the other writing systems, added 2026-09-26.** `cjk.rs` ports
  `afcjk.c` (and `afindic.c`, the same without zones) for ideographs, the
  four Indic-stub scripts and the *fallback style* -- the glyphs no script
  claims: a Latin font's arrows, mathematical signs and `.notdef`, and every
  glyph of a face without a Unicode `cmap`. FreeType's dummy system is
  ported too, for what it does to a glyph it will not hint (a Latin style
  whose zones cannot be measured): each point still goes from whole font
  units to 1/64 pixel. So every glyph FreeType draws, this draws the same.
  Where FreeType's code does something other than what it says, the port
  follows what it does: its CJK pass that judges segments round never runs
  (it reads the segment count before the Latin pass fills the table), so
  the Latin roundness stands. Found by building FreeType with its
  auto-hinter's debug dumps (`FT_DEBUG_AUTOFIT`) and comparing edge tables;
  that single difference moved Malgun Gothic's stems 3/64 pixel on 2,925
  glyphs.
* **A variable font's points are FreeType's too, added 2026-09-26.** At a
  non-default instance the hinter reads each glyph as FreeType's TrueType
  loader hands it to FreeType's hinter, not as this crate draws it: the
  instance normalized in 16.16 as `ft_var_to_normalized` does (a
  `Coords` now carries that beside HarfBuzz's `F2Dot14`), each `gvar` delta
  summed in FreeType's fixed point and rounded to a whole unit per point
  (`TT_Vary_Apply_Glyph_Deltas`), component offsets likewise, scaled
  components through `FT_MulFix`, and the glyph moved by its left phantom
  point -- which `gvar` moves only in a face without `HVAR`
  (`Face::load_unscaled`, `Gvar::deltas_fixed`, `ftcalc`). Drawing keeps
  its exact, HarfBuzz-following outlines; only the hinter's input changed.
  Before, 31% of Noto Sans's glyphs at a bold condensed instance differed
  from FreeType's, a few by a whole pixel; now none do, at fifteen
  instances of eight variable fonts (Segoe UI Variable with `avar` and
  `opsz`, Bahnschrift, Cascadia Code, Sitka, Reem Kufi, Noto Sans, Open
  Sans, JetBrains Mono). The fixture gained a variable face, with `HVAR`
  and without, whose weight-610 deltas all land on fractions; both halves
  were mutation-checked (2.14 coordinates, and phantom moves under `HVAR`).
* **Checked against FreeType.** `tools/hint_oracle.py` runs FreeType (from
  `freetype-py`) and this crate over every glyph of a face at eleven sizes
  and compares every hinted point, both coordinates, to the 64th of a pixel,
  after comparing the sorting itself against FreeType's own glyph-to-style
  map. On Noto Sans, Open Sans, JetBrains Mono,
  Segoe UI, Segoe UI Symbol, Arial, Times New Roman, Calibri, Consolas,
  Verdana and Georgia every glyph lands in FreeType's style and every glyph
  agrees exactly, the fallback style's symbols included -- and so does every
  glyph of the CFF fonts David CLM and Frank Ruehl CLM, whose coordinates are
  fractions of a unit, and of the CJK fonts Malgun Gothic, Microsoft YaHei,
  MS Gothic, SimSun, SimSun-ExtG, Yu Gothic, Microsoft JhengHei and Noto
  Sans JP (CID-keyed CFF): over a million glyph renderings, none different. That last took reading a glyph's
  points as FreeType's loaders read them, not as its outline draws: a CFF
  coordinate kept exact (16.16 needs more than `f32` has) and floored to a
  whole unit, a line of no length in 1024ths of a unit dropped, a contour
  that ends a hair short of its start folded (`sfnt::CffPoints`), and a
  composite placed by the component whose metrics it borrows. A generated
  fixture (`tools/gen_hint_fixture.py`: a synthetic face as TrueType and CFF,
  one glyph drawn in 16.16 fractions, small capitals and superscripts behind
  `smcp` and `sups`, ideographs drawn for each rule of the CJK system, with
  FreeType's answers at eighteen sizes) keeps that in `cargo test`.
* **Robust before faithful.** Every index goes through `get` and a failure
  abandons the glyph to be drawn unhinted, coordinates beyond `i16` and
  absurd sizes are refused at the door (which is what makes the unchecked
  fixed-point arithmetic provably safe), and every walk of font data is
  budgeted. A bug in the port costs a glyph its hinting, never the compositor.
* **Licensing.** FreeType's licence (FTL) permits this with a credit line; the
  licence and the credit are in `gui/font/licenses/`, and every ported file
  carries FreeType's copyright notice.

**Not done:** stem darkening (off by default in FreeType too). The CJK
writing system, first listed here, was ported the same day. The oracle also
turned up a difference that was not hinting's: composites that borrow a
component's metrics were placed a few units off FreeType horizontally, hinted
or not -- fixed. The face-level analysis -- styles and zones, 2-9 ms on large
fonts, taken when a face first draws a hinted glyph -- is repeated for each
size of a face; sharing it is an optimisation for later.

**Alternatives.**

| | For | Against |
|---|---|---|
| An original light hinter | smaller, no licence | tried: different text from FreeType's, no oracle to find out why |
| TrueType bytecode interpreter | the designer's own hints | §86c: runs attacker-supplied programs; nothing for CFF or unhinted fonts |
| DirectWrite-style (Windows) | the other big reference | not documented to the level a port needs; no oracle to check against |
| No hinting | nothing to maintain | soft small text on ordinary monitors, the setting a lie |

**How to reverse.** The setting off draws exactly as before; `Rendering`'s
`hinting` defaults to off for any caller that does not ask. Removing the
module is removing `ScaledFont`'s `hinter` field.
