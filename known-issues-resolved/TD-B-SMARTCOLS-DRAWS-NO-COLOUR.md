## TD-B-SMARTCOLS-DRAWS-NO-COLOUR (lane B, 2026-10-17)

**Status:** FIXED 2026-10-22 (lane B). `smartcols` writes cells' colours as
libsmartcols 2.39.3 does: `Table::enable_colors`, a header cell's colour
(`column_header_set_color`, a sequence or a colour's name), and the printer's
`fputs_color_cell_open`/`_close` and line open/close around each cell --
after the width calculation, which they do not touch. fdisk-list's two
tables set their headers in the scheme's `header` colour when colours are
wanted, so `fdisk` and `sfdisk` print them bold as Ubuntu's do
(`fdisk-diff.sh`'s `--color=always` cases). Lines' and columns' own colours
are not kept: nothing here sets one.

**In short:** util-linux's table library colours what a program asks it to
-- a column's header in bold, a cell in a colour of its own -- when the
output is a terminal that wants colour. The port of that library
(`userspace/smartcols`) has no colour at all, so `fdisk`'s and `sfdisk`'s
partition tables print their headers plain on a colour terminal, where
Ubuntu's are bold. Nothing else differs, and no output that is not a
terminal is affected.

**Where:** `userspace/smartcols` has no `scols_table_enable_colors`,
`scols_cell_set_color` or `scols_line_set_color`, and its printer writes no
escape sequences. Callers that would use them: `userspace/fdisk/src/list.rs`
(`new_table`: upstream sets each header cell to `color_scheme_get_sequence
("header", UL_COLOR_BOLD)` when `colors_wanted ()`), and later sfdisk and
cfdisk.

**The proper fix:** port libsmartcols' colour handling (`lib/colors.c`'s
sequences are already in `ulcolors`): the table's `colors_wanted` flag, a
colour per column, line and cell, and the printer's `fputs_color_cell_open`
/ `_close` around each cell's data -- which the width calculation must leave
out, as upstream's does. Then set the header colours in fdisk-list's tables.
