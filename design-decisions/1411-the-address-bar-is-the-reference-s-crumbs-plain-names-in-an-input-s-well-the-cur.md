## 1411. The address bar is the reference's crumbs: plain names in an input's well, the current folder bold

**Date:** 2026-09-27 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The toolkit's address bar (`guitk::pathbar` -- the explorer's,
and the file-types program's) drew each folder of a path as a rounded box with
a ">" between them. It is the Aero reference's now: the folder names as plain
text in a text field, small drawn chevrons between them, the folder you are in
set bold -- and the same field whether it shows the path as crumbs or as text
to edit. Two bugs came out with it. Clicking a folder after the "..." of a
long path went to the wrong folder -- the first one after it went to the root
-- because the crumbs on screen were numbered from the first one drawn rather
than from the path's start. And a click in the typed path put the caret half a
character right of where it was aimed, the text being drawn four pixels
further right than the click was measured from.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| The field | the palette's `crust`, where every text input sinks, with a `surface1` edge; the same in both modes | the reference's white field, lighter than its bar | the palette has no "lighter than its bar" role in light mode, and a field that keeps the input convention reads as an input under every theme; changing colour on entering edit mode would read as a second control appearing over the first |
| Between two crumbs | a chevron drawn as two strokes, in `overlay0` | a `>` glyph | the reference draws one, and a glyph's shape depends on the font installed -- the tree view draws its arrows the same way |
| Size | 13px, the toolkit's body | the reference's 12.5 | edit mode is set at the same size, so the path does not jump when the trail turns into text |
| A current folder too long for the bar | drawn anyway, cut short with an ellipsis | dropped behind the "...", as before | the folder you are in is the one crumb that has to be on screen |
