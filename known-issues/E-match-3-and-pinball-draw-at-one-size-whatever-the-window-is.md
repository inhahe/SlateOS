### [E] Match-3 and pinball draw at one size whatever the window is -- 2026-09-28

**Status:** open.

**In short:** the match-3 game lays its board out in fixed pixels -- 48-pixel
cells (`CELL_SIZE`, `apps/match3/src/main.rs`), a window size computed from
them -- so in a window larger than that the board sits in a corner of empty
page, and in a smaller one it is cut off. A gem's symbol is placed by
eyeballed offsets from the cell's middle (`CELL_SIZE / 2.0 - 6.0`, `- 8.0`),
right for one font at one size. Pinball is the same: its table, sidebar
and footer are laid out at `WINDOW_WIDTH` x `WINDOW_HEIGHT` and translated to
the middle of a larger window (`Pinball::frame_at`), so a larger window
shows the same small table in a margin of page and a smaller one crops it.
The operator asked that the games "fit every size" (C-Q16, §1422); both were
themed on 2026-09-28 without this.

**The proper fix.** A `Layout` solved from the window's size, as sudoku's
and crossword's are: the cell size the largest that fits the board and its
header and footer, everything placed from it, the hit test reading the same
layout, and the symbol centred by measuring it (`guitk::text::measure`,
`line_height`). Tests over a range of window sizes, as those games carry:
nothing drawn outside the window, and every gem's hit box on its gem.
