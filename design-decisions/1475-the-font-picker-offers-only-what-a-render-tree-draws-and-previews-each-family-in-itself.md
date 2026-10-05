## 1475. The font picker offers only what a render tree draws, and previews each family in itself

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the toolkit has a font picker now (`guitk::fontpicker`): the
families this machine has, each name drawn in its own face; the weights; the
size; a line of preview; and the colour picker's three events, so a program
can preview a choice live and keep or revert it. It offers regular and bold
only -- and bold only where the family has one -- and whole-pixel sizes,
because that is all a program can draw today; a picker that listed "Light"
or "Italic" would show them in regular and the program would draw them that
way.

**Where:** `gui/toolkit/src/fontpicker.rs` (`FontPickerDialog`,
`FontChoice`, `FontPickerEvent`), `gui/toolkit/src/fontdb.rs`
(`FaceInfo::weights`, `FontDb::has_bold`), `guitk::text::family_has_bold`.
Asked for by `roadmap-detailed.md` §3.5 (*Font picker dialog*, with *live
"tentative selection" events*). It draws families by name through
`FontFamily::Named` (`requests/c-f-text-in-a-family-the-drawing-names.md`),
so its rows show each face once lane F's half of that pair is in; until then
the compositor draws them in the UI face.

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Only the weights a render tree draws** -- regular, and bold where the family has a bold face or a variable one reaching 600 | the family's real styles (Light, Semibold, Italic...) | A choice the program cannot draw is a choice that changes nothing: the preview would show "Light" in regular, and so would the document. `FontPickerDialog::weights` is the one place that grows when the render tree carries more (`requests/c-f-text-at-any-weight-and-in-italic.md`). | A family like Segoe UI shows two of its eight styles until then. |
| **Whole pixels, 1 to 512** | fractional sizes, or points | The font cache rounds every size to a whole pixel and builds nothing past 512 (`osfont::system::MAX_PX`): a picker that took 10.5 would draw 10 or 11. The toolkit's sizes are pixels throughout, as `fonts.ui_size` is. | A document format that thinks in points converts on its side. |
| **The colour picker's three events** (`Changed`, `Confirmed`, `Cancelled`) | a picker that reports only its result | `roadmap-detailed.md` asks for live tentative selection, and the colour picker already speaks it: a host previews and reverts both the same way. `Cancelled` leaves `choice()` the original, so a host that previewed puts that back. | Every step through two hundred families is an event; a host that redraws a large document on each must keep that cheap. |
| **Each family's row drawn in its face** | the names in the UI face | The name set in the face is the quickest preview there is, and lets a list of hundreds be read at a glance. | Each row loads its family's faces, in the program (to measure the row) and in the compositor (to draw it): a first scroll through the list reads the files. The cache holds 32 named families (`MAX_NAMED_FAMILIES`), so a long scroll reloads. |
| **Search: names that start with the text first, then those that contain it; the choice kept while listed, else the first listed chosen** | substring order alone, or a search that never chooses | Typing "ar" should reach Arial before Candara, and typing a family's name should choose it, as a font box does. Keeping a choice the search still lists stops a refinement from jumping the selection about. | Typing makes tentative choices, one per keystroke that lands on another family. |
