### [E] The terminal draws no combining mark -- 2026-09-28

**Status:** open.

**In short:** text with an accent written as a separate mark -- "e" followed by
U+0301, as macOS writes file names and many programs print -- shows as a plain
"e" in the terminal. `put_char` (`apps/terminal/src/lib.rs`) gives a mark no
cell, which is right, and then does not keep it anywhere, so nothing draws it.
The cursor and the width query both count the mark as taking no cell, so
nothing is misplaced; the accent is simply missing, and a copy of the line
loses it too.

**The proper fix.** A cell keeps the marks that follow its character (a short
list on `Cell`), the glyph call draws the cell's whole cluster clipped to its
cells, and a selection's copy carries the marks. Whether the font places the
mark over its base is the text layer's (`GPOS` mark attachment, which
`gui/toolkit`'s shaper has); the terminal's part is to keep the mark and hand
it over.
