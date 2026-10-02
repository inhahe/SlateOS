## C-SPREADSHEET-SCROLLBARS-ARE-DRAWN-BUT-NOT-DRAGGABLE (lane C, 2026-08-20) -- **fixed 2026-09-03**

**Status:** fixed — see "Fixed 2026-09-03" at the end of this entry.

`apps/spreadsheet/src/main.rs` draws a vertical and a horizontal scrollbar
(`render_scrollbars`, the `scroll_ratio` computations near the end of the
render path). Both track the offset correctly, so they always *show* the right
thing. Neither is wired to the mouse: `handle_left_click` tests for sheet tabs,
column/row resize handles, the autofill handle and then the grid, and returns
`Ignored` for a click anywhere in the scrollbar gutter. There is no thumb-drag
state and no `MouseEventKind::Move` arm for one.

So the grid can be moved by the wheel, by the arrow keys, by Page Up/Down and
by anything that calls `ensure_cell_visible` — but not by the control that
exists to move it, which is the first thing a user will reach for.

**The fix** is a drag mode alongside the existing `InteractionMode::ColResize`
/ `RowResize`: press in the gutter jumps a page in that direction, press on the
thumb records the grab offset, move maps the pointer back through the same
`scroll_ratio` the renderer used, and release ends it. The mapping must be the
renderer's own ratio rather than a second derivation of it — that is exactly
the divergence the layout law (`col_screen_x`/`row_screen_y`) was introduced to
end, and a scrollbar with its own idea of the offset would reintroduce it in a
new place. Note `scroll_by` already screens non-finite input, which a drag
needs because the ratio divides by a content extent that can be zero.

### Fixed 2026-09-03

Done as the entry describes. `ScrollbarGeometry` is the single law: given a
track, an axis, a `max_scroll` and an offset it produces both the thumb's rect
and, through `offset_at`, the inverse map from a pointer position back to an
offset. `render_scrollbars` draws what it returns and `drag_scrollbar` reads
the pointer through it, so the drawn thumb and the drag cannot disagree —
`tests::the_thumb_placement_and_the_offset_map_are_inverses` pins the two as
inverses across the whole range rather than at one point, since a wrong scale
still agrees at zero. `InteractionMode::ScrollDrag { vertical, grab }` holds
the grab offset so a thumb caught near one end does not jump its centre under
the pointer. A press in the gutter pages towards the press and starts no drag.

**Three pre-existing bugs surfaced while testing this, all now fixed.** Each
was a second derivation of a number that already had one — the same failure
`col_screen_x`/`row_screen_y` was introduced to end — and in each the error was
invisible at zero and grew with the offset:

- **The horizontal bar was drawn over the sheet tabs and could never be
  clicked.** `grid_height` reserved no band for it, so `grid_bottom()` was the
  tab strip's top edge, while `grid_width` had always reserved
  `SCROLLBAR_WIDTH` for the vertical bar. The bar was unreachable twice over:
  the tabs are drawn after it, so they painted over its top half, and the tabs
  are hit-tested *first*, so a press on what was left picked a sheet. Fixed by
  making `grid_height` reserve the band symmetrically with `grid_width`.
  Pinned by `tests::the_horizontal_scrollbar_does_not_overlap_the_sheet_tabs`,
  which checks it at three window sizes.

- **With the status bar hidden, every sheet tab was dead.** The click test
  computed the strip's top as `window_height - SHEET_TAB_HEIGHT -
  STATUS_BAR_HEIGHT` unconditionally; the renderer subtracted
  `STATUS_BAR_HEIGHT` only when the bar was actually shown. With it hidden,
  every tab's hit box sat a status bar's height above the tab drawn for it —
  so the visible tabs did nothing and a blank band of grid selected them.
  Fixed with one `tab_top()` law, used by both.

- **Clicking a tab's right-hand edge selected the tab next door.** The
  renderer laid the tabs out from `x = 4` in strides of `SHEET_TAB_WIDTH + 2`;
  the click test sliced the strip from `x = 0` in strides of
  `SHEET_TAB_WIDTH` alone. 4px out at the first tab, a further 2px at each one
  after it — and by the "+" button, 14px, wider than half the button, so the
  button could not be hit at all. Fixed with `sheet_tab_rects()`, one walk
  yielding a rect per tab plus one for the "+", which the renderer draws and
  the click test hit-tests. Pinned by
  `tests::every_tab_selects_itself_along_its_whole_width`, which clicks each
  tab's left edge, centre and right edge — the drift is largest at the right
  edge and grows with the index, so a single tab at its centre would have
  shown nothing.

The two tab tests take their click coordinates **out of the render command
list**, not from `sheet_tab_rects()`. Asking the law under test where the tab
is would make the click and the drawing agree by construction and the tests
could not fail. All three were mutation-checked: each of the three fixes was
reverted in turn and the corresponding test observed to fail with the
predicted numbers (`the bar ends at 762 but the tabs start at 748`; `x=93 is
drawn inside tab 0 but selected sheet 1`; `clicking the second tab where it is
drawn selected sheet 0`).

241 tests pass; clippy and rustfmt clean.
