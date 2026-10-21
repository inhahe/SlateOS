# E -> F: `osfont::testing` could build a face with a family name

**From:** Lane E (`apps/fontmanager`). **To:** Lane F (`gui/font`).
**Filed:** 2026-10-10. **Status:** OPEN -- nothing breaks while it waits;
lane E's tests carry a copy of the fixture.

**In short:** `osfont::testing::colour_face` builds a complete font byte by
byte, but with no `name` table, so the toolkit's font index
(`guitk::fontdb`) skips it -- an index is keyed by family name. The Font
Manager's tests need faces the index lists: a family name, a weight and
slant, fixed pitch or not. They build them in
`apps/fontmanager/src/library_tests.rs` (`font`, `named_font`) by copying
`colour_face`'s tables and `assemble`, and adding `name`, `post` and
`head.macStyle` -- a second copy of a font-format fixture, outside the crate
that owns the format.

## What it asks

`osfont::testing::named_face(names: &[(u16, &str)], bold: bool, italic:
bool, fixed: bool) -> Vec<u8>` (or whatever shape suits): `colour_face`'s
glyphs without the colour tables, a `name` table of Windows Unicode records,
`head.macStyle` for bold and italic, and `post.isFixedPitch`. Lane E's copy is
there to lift.

## What lane E then does

The Font Manager's tests build their faces with it (`osfont` with the
`testing` feature in `[dev-dependencies]`), and their copy goes.
