### TD-FONT-NO-CFF-OUTLINES. `osfont` cannot draw any `.otf` whose outlines are PostScript/CFF rather than TrueType — 2026-08-13 — ✅ FIXED 2026-08-14 (`gui/font/src/cff.rs`, `gui/font/src/sfnt.rs`, `gui/font/src/raster.rs`)

**What it was.** `gui/font/src/sfnt.rs` parsed the sfnt container, `cmap`,
`hmtx`, `loca` and `glyf`. Outlines in the `CFF `/`CFF2` tables are a
completely different representation — a Type 2 charstring stack machine, not a
point list — and were not implemented. `Face::parse` detected them and returned
`SfntError::CffOutlinesUnsupported`, so **18 of 556** fonts on this host would
not open at all: ~3% of a typical Windows font set, but a 3% that includes most
Adobe faces and *all* of `.otf` in the colloquial sense.

**Fix, in two commits so each was independently verifiable.**

1. `d02567ebb` added `PathCmd::CurveTo` and a cubic flattener to `raster.rs`,
   with `cubic_segments` deriving its subdivision count from the same chord-
   error bound as the quadratic path (a cubic's second derivative is three
   times a quadratic's, hence the constant 27 against the quadratic's 3 inside
   a fourth root). This was proved **before any CFF parser existed**, using
   degree elevation as an exact oracle: every quadratic *is* a cubic whose
   controls sit two-thirds of the way from each endpoint to the quadratic's
   control, so the new code path had to agree with the already-proven one to
   within flattening error and nothing else. When CFF glyphs later came out
   looking right, the flattener was already known-good.
2. `3b8be9f4d` added `gui/font/src/cff.rs`: INDEX/DICT parsing (including
   packed-BCD reals and the 1-based-from-the-preceding-byte offset convention),
   then the charstring machine — every path operator, stem counting so
   `hintmask` skips the right number of inline bytes, local and global
   subroutines with the count-dependent bias, all four flex operators, `seac`
   accent composition, CID `FDSelect` for per-glyph Private DICTs, and
   FontMatrix reconciliation against `head.unitsPerEm`.

**Measured after.** Same command as before
(`cargo test -p osfont --target x86_64-pc-windows-gnu --test host_fonts --
--ignored --nocapture`): **556/556** fonts open, **18/18** `.otf` files
rasterize, 708,793 ink pixels from CFF faces. A new
`cff_letters_look_like_letters` prints glyphs from two CFF faces and checks
them structurally, because the aggregate ink counts would pass just as happily
for a mirrored glyph or one wound so the counter fills instead of cancelling.

**Still deliberately unsupported**, each returning
`SfntError::CffUnsupported(&'static str)` naming the construct rather than
drawing a plausible wrong glyph:

* **CFF2** — the variable-font revision. Structurally similar to CFF (no Name
  INDEX, blend operators, an item-variation store), which is exactly why
  running it as CFF would misread it rather than fail. Implementing it means
  implementing variations, which nothing needs yet. *(Read since 2026-09-26:
  see "[F] A variable font whose outlines are CFF (`CFF2`) would not open".)*
* **Type 1 charstrings in a CFF wrapper** — the operator sets overlap but mean
  different things.
* **The Type 2 arithmetic operators** (`add`, `div`, `random`, the transient
  array, …). No shipping font uses them for outlines; they exist for
  procedural effects.
