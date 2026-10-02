### [F] A variable font whose outlines are CFF (`CFF2`) would not open -- 2026-09-26 -- **FIXED 2026-09-26**

**Status:** FIXED 2026-09-26 (lane F) — `gui/font/src/cff.rs` reads `CFF2`
(`Cff::parse2`) and its charstrings' `blend`s, as each library weighs them
(design-decisions §1328).

**In short (as found):** a variable font whose letters are drawn as
PostScript curves rather than TrueType ones -- Adobe's Source Sans, Serif and
Code, the variable builds of Noto Sans CJK and Source Han -- was refused
outright: `Face::parse` returned `CffUnsupported("CFF2 table")`, so the font
could not be used at any weight, not even its default. None is installed on
this host, which is why nothing had asked.

**What `CFF2` is.** The same charstrings as CFF in a leaner container -- a
header giving the Top DICT's length, INDEXes counted in 32 bits, always an
FDArray (FDSelect optional, and with a third format), no charset, no widths,
no `endchar` -- plus an item variation store, and two operators: `vsindex`
chooses one of the store's subtables, and `blend` turns each of `n` values
into its default plus one delta per region of that subtable, weighed at the
instance. Nothing else in a glyph varies; advances come from `HVAR` as
before.

**Each library's arithmetic.** The deltas are weighed differently by the two
libraries the crate follows, and a glyph is read both ways: as HarfBuzz reads
it for drawing and measuring (`f32` region scalars at its `F2Dot14`
coordinates, the weighed deltas summed in `f64`), and as FreeType reads it
for the hinter (a blend vector of `FT_DivFix`ed axis factors multiplied by
`FT_MulFix`, at its 16.16 coordinates, each delta `FT_MulFix`ed into a 32-bit
16.16 sum). Where the two refuse a malformed blend differently, each refuses
as its library does.

**CFF boxes, too.** HarfBuzz measures a CFF glyph by its path in `double`
and in charstring units -- it reads no `FontMatrix` -- where the crate took
the box of its `f32` outline, scaled by the matrix. Both CFF and `CFF2`
boxes are now HarfBuzz's (`Cff::bounds`, `Face::glyph_extents_at`).

**Checked:** `tools/outline_oracle.py` (new) against HarfBuzz's drawing and
boxes, and `tools/hint_oracle.py --var` against FreeType's hinting, on four
real `CFF2` fonts at named and in-between instances -- Source Sans 3 VF,
Source Code VF, Source Serif 4 Variable (two axes, six Font DICTs) and Noto
Sans JP VF (CID-keyed, eighteen Font DICTs): every path, box and hinted
point agrees (26,651 glyph-instances drawn; 7,407 and more hinted glyphs per
instance). The hint fixture gains a `CFF2` face built by fontTools with
fractional deltas, pinning FreeType's hinting of it and HarfBuzz's drawing.
