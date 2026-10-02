## 1204. A spreadsheet is a tab-separated workbook file, `.spreadsheet`, read whole or not at all; a CSV opens as a new workbook

**Date:** 2026-09-25
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** The spreadsheet can now keep a whole workbook -- every sheet,
every cell as it was typed (so a formula comes back a formula), every format,
column width, row height and frozen pane -- in a file of its own, and open it
again. Before, Ctrl+S wrote the sheet in front as CSV: its values only. The
file is plain text, one line per cell, fields separated by tabs. A file with a
line this version does not understand is refused whole, saying which line.
CSV is still read and written: Ctrl+O on a CSV opens it as a new workbook of
one sheet, and Ctrl+E exports the sheet in front as CSV.

### Tab-separated lines rather than YAML

The slide deck, the diagram and the whiteboard are YAML (`yamldoc`), and the
design's rule is YAML for anything a person might edit. The spreadsheet does
not follow them, for two reasons:

| | Tab-separated lines (chosen) | YAML (`yamldoc`) |
|---|---|---|
| A full sheet (26 x 999 cells) | one pass to write, one to read | `yamldoc` finds each key by scanning the document; tens of thousands of cells make that quadratic -- minutes, not milliseconds |
| Reading it elsewhere | `cut`, `grep`, any spreadsheet's text import | a YAML library |
| Consistency | the finance ledger's format (§1202): the same four escapes, the same all-or-nothing reading | the other document editors' |

The first is the deciding one: a format that takes minutes to open a full sheet
is not a format.

### What a line holds

`slateos-spreadsheet<TAB>1` first; then `sheet<TAB>name`, and under it
`frozen<TAB>rows<TAB>cols`, `width<TAB>C<TAB>px`, `height<TAB>7<TAB>px` (only
what differs from the default), and `cell<TAB>A1<TAB>what was typed` followed by
its format as flags -- `bold`, `italic`, `align=right`, `number=currency:2`,
`text=#RRGGBB`, `fill=#RRGGBB`, `border=tl` -- nothing for a default; last,
`active<TAB>n`. A tab, a line break or a backslash in a name or a cell is written
`\t`, `\n`, `\r` or `\\`.

### Read whole or not at all

A line not understood, a cell written twice, a broken escape, a later format
number: each refuses the file, and the message names the line. The diagram and
the whiteboard instead leave out a shape they do not understand and read the
rest (as the slide deck does). The difference is the ledger's argument: a
workbook read in part and saved again would silently lose what was not read,
and in a workbook that is somebody's numbers.

### A CSV is not the workbook's file

Opening a CSV makes a new workbook from it and leaves the workbook with no file,
so Ctrl+S asks where to save -- it never writes a workbook over the CSV. The
alternative, writing the CSV back, loses every formula, format and other sheet
the user adds, which is the failure this change exists to end. The workbook is
named `.spreadsheet`, the pattern the other editors use (`.slides`,
`.diagram`, `.whiteboard`). Save As is F12 -- Excel's key -- because
Ctrl+Shift+S already shows and hides the status bar here.

**Where it lives:** `apps/spreadsheet/src/main.rs`: `workbook_text`,
`parse_workbook`, `format_flags`, `apply_flag`; `SpreadsheetApp::{write_workbook,
open_path, save, picked, unless_unsaved, request_close}`. The escapes are
`textfmt::tsv`'s since the same day, shared with the ledger and the notes
library (§1205).

**How to reverse:** the format is `workbook_text`/`parse_workbook`; a YAML file
replaces those two, and keeps `open_path`'s rules about CSV and about reading
whole.
