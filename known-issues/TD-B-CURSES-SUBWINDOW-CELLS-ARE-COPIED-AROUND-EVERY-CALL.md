## TD-B-CURSES-SUBWINDOW-CELLS-ARE-COPIED-AROUND-EVERY-CALL (lane B, 2026-10-21) — OPEN

**Status:** OPEN

**In short:** a curses sub-window (`subwin`, `derwin`) keeps a copy of the
part of `stdscr` it covers, and every call made on it copies that block in
from `stdscr` first and back out after. So drawing a window line by line --
cfdisk's extra-information window, one `mvwaddstr` per row -- costs rows²
times columns cell copies instead of rows times columns. Nothing is wrong on
screen, and at terminal sizes the cost is microseconds; it grows with the
window.

**Where:** `userspace/curses/src/windows.rs`, `with_win` (the copy in and
out around each call on a `Win::Sub`) and `copy_in`/`copy_out`. Found by
Fable's review of `1824ee3c3..759e931a6` (batch 8).

**Measured shape:** a 20×78 window drawn a row at a time is 20 full passes
of about 1,560 cells, some 31,000 copies per redraw, repeated on every cursor
move in cfdisk.

**The proper fix:** make a sub-window a view of its parent's cells, as
ncurses' sub-windows share their parent's line memory: the window keeps its
origin and size, and its drawing calls address `stdscr`'s cells through
that offset. That removes `copy_in`/`copy_out` and the quadratic term
together. It touches every drawing call that takes a `Win`, which is why it
was not folded into the review follow-up. A smaller step that also removes
the quadratic term: copy in once at the first call on a sub-window after
`stdscr` last changed, and out at `wnoutrefresh`.
