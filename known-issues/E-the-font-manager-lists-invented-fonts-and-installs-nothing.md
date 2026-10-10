# The Font Manager lists invented fonts, and installs nothing

**Status:** OPEN (lane E, found 2026-10-10)

**In short:** the Font Manager (`apps/fontmanager`) shows nineteen fonts it
made up, whatever is installed, and its Install and Remove change only that
list: no font file is copied anywhere, so a font "installed" there is drawn
by nothing and gone when the program closes. A user who installs a theme's
recommended font with it -- the Fonts page's natural next step -- is told it
worked, and nothing changed.

**Where:** `apps/fontmanager/src/main.rs`.

- `FontCollection::add_default_fonts`, called by `new_with_defaults`: nineteen
  `FontInfo`s written into the source -- families, styles, paths and a
  `glyph_count` of 200 -- with nothing read from the machine.
- `FontCollection::install(path)`: takes the family from the file's name,
  checks the made-up list for a duplicate, and pushes a `FontInfo` with
  `category: SansSerif`, `version: "1.0"` and `glyph_count: 200` -- invented
  -- and never opens the file. `uninstall` removes the entry and no file.

**How to see it:** run it; the list is the same on every machine. Install
any path ending `.ttf`, even one that does not exist: it is listed, and
`guitk::text::family_installed` still says no.

**The proper fix:** the program over the machine's real fonts -- the
toolkit's font index (`guitk::text::available_families`, the index's faces
and files) for the list, a face read with `osfont` for the preview and the
details; Install copying the file into the user's font folder, where the
index looks, after reading it as a font (refused, and why, if it is not
one); Remove deleting a font of the user's own, and refusing a system one
with the reason. Lane E's.
