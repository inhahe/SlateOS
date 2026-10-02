### [F] A variable font whose `avar` is version 2 loses its whole axis correction -- 2026-09-26 -- **FIXED 2026-09-26**

**Status:** FIXED 2026-09-26 (lane F) — both readings read version 2:
HarfBuzz's through `VarStore` and `IndexMap` at `roundf(c / 4)`, its
rows summed through HarfBuzz's scalar cache (`var.rs` `hb_avar2`); FreeType's
through `FtItemStore`, with FreeType's own refusals, its axis map read only
after a clean store load, and `delta << 2` added in 16.16 (`ft_avar2`).
`tools/gen_var_fixture.py` builds a face with two cross-axis mappings and
records both libraries' answers at 108 instances; every one agrees
(design-decisions §1326). Doing it found that the HarfBuzz reading had not
been HarfBuzz 14.3.0's for any face -- the entry below.

**In short (as filed):** a variable font that ships the newer version of its weight
correction table (`avar` 2) is drawn as if it had no correction at all, so
"Semibold" can come out lighter or bolder than the designer drew it. No font
installed here uses version 2, which is new (2023), so nothing on screen is
wrong today.

**Where.** `gui/font/src/var.rs`, `parse_avar` and `parse_avar_pairs`: each
returns nothing for any version but 1. Version 2 keeps version 1's segment
maps at the same place and adds an item variation store (and a delta-set
index map) whose deltas are added to each normalized coordinate afterwards.
Dropping the table drops the segment maps too.

**The proper fix.** Read version 2's segment maps as version 1's, then the
axis-index map and item variation store (`varstore.rs` reads the same
structures for `HVAR`), and add each axis's delta: in `F2Dot14` for the
HarfBuzz path (`hb-ot-var-avar-table.hh`), and in 16.16 for FreeType's
(`ft_var_to_normalized`: `v += delta << 2`, clamped to ±1). Check both
against their libraries with a font fontTools builds with a designspace
`<mappings>` element, since no installed one has the table.
