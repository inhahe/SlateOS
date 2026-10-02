## `TD-C-EXPLORER-DOES-NOT-SCROLL` (lane C, 2026-08-26) -- **CLOSED 2026-09-07**

**Closed 2026-09-07 (lane C).** All three views scroll. The narrowing below
says the icon grid did not yet; it does now, and the narrowing is the stale
part.

`render_icon_view` reads `viewport.first_visible()`, rounds it down to a whole
row of icons (starting mid-row would put the first cell in the middle of the
pane with a gap beside it), takes `scroll_window::capacity` rows' worth of
cells, clips to the pane so a partial bottom row is cut mid-cell rather than
vanishing, and lays each cell out by its *visible* position while identifying
it by its *absolute* index -- so drop zones land on the file that was drawn.

Four tests cover it: `the_icon_grid_scrolls_and_reaches_the_last_file`,
`a_scrolled_icon_cell_names_the_file_that_is_drawn_in_it`,
`a_wheel_notch_in_the_grid_moves_a_whole_row_of_icons`, and
`the_icon_grid_wraps_and_survives_a_pane_narrower_than_a_cell`.

Original entry and its narrowing follow.

---


**Narrowed 2026-09-07: the two list views scroll; the icon grid does not yet.**
`ExplorerState` holds a `ListViewport`, the details and list renderers draw the
rows `scroll_window::visible` names, the wheel routes through
`wheel::Accumulator`, and `move_selection_to` syncs the cursor row into the
viewport so the arrow keys drag the view with them. `ListViewport` gained
`scroll_by`, which is the one operation there that leaves the selection alone --
a wheel that revealed the selection could not scroll at all, because the view
would snap back on the next call.

**The icon grid scrolls too, as of the commit after that one.** The offset
stays in *entries* -- one number for all three views, so changing view mode
lands you where you were rather than at the top -- and the grid rounds it down
to a whole row of icons, because starting mid-row would put the first cell in
the middle of the pane. The wheel multiplies its row step by the column count
there: a notch is three rows, and in a grid a row is `cols` entries, so
stepping by entries would move three files and look unresponsive.

**CLOSED 2026-09-07.** The scrollbar landed with the commit after that one:
a track and a draggable thumb in all three views, a page jump for a click on
the track either side of it, and no bar at all when the listing fits.

It is drawn from a new shared `guitk::scrollbar`, not a seventh private copy.
Six places in the tree already had one, each with its own version of the same
formula. Five now share this module -- the file dialog, `menu`, `menubar`, the
desktop shell and `apps/dictionary`; the sixth, `apps/spreadsheet`, is
deliberately left because its bar is generic over the axis and this module is
vertical-only. (The commit named `window_peek` as one of the six. That file has
no scrollbar: the grep had matched `max_thumb_height`, which sizes a window
preview. See the module doc.) The extraction took the file dialog's, which
was the best documented and the only one with a test for the end-of-list case,
and `dialog.rs` was converted first so its own tests prove the shared module
says what its copy did.

**Follow-up worth doing, not done here:** the other five copies. They are
independent of explorer and each is a small, separately-testable conversion;
folding them into this change would have made a scrolling fix into a
five-file refactor. `move_selection` also still exists
rather than deferring to `ListViewport::select_prev`/`select_next` as the plan
below suggests -- explorer's multi-selection means the viewport is used for
scrolling only, and folding the two together is the part that needs the
anchor/extend decision the "one genuine mismatch" section describes.


**In short:** the file manager draws as many entries as fit in the window and
then stops. There is no scrollbar, no mouse wheel, no Page Up/Page Down — so in
a directory with more files than fit on screen, the ones past the bottom edge
**cannot be reached at all**. The arrow keys will move the selection onto them
(the selection is an index into the full listing, not into the visible part), at
which point the highlighted row is off-screen and the window appears to have
lost the selection.

**Where it lives.** `apps/explorer/src/main.rs` — the renderer walks
`self.entries` and emits rows until it runs out of vertical space; nothing holds
a scroll offset. `dropzone.rs` registers hit rectangles only for the rows that
were actually drawn, which is correct and means clicking is consistent with what
is visible — the gap is purely that nothing changes *which* rows those are.

**Why it is separate from the input work.** Scrolling is a viewport, not a key
binding: it needs a first-visible-row offset threaded through all three view
layouts, a scroll-into-view rule so keyboard movement drags the viewport with
it, wheel handling in `handle_mouse` (`MouseEventKind::Scroll` is delivered and
falls into the `_ => false` arm), and a scrollbar. That is a layout change, and
folding it into the commit that made the window clickable would have made both
harder to review.

**What the proper fix is — and it is mostly *use what is already there*.**
`gui/toolkit` has this solved, because a dozen settings panels hit it first:

| Piece | Where it already lives |
|---|---|
| Offset + selection kept in agreement, so the picked row is never off screen | `guitk::listview::ListViewport` — `select_prev`/`select_next`/`page_up`/`page_down`/`set_height`/`visible_range`, all clamped against a `len` passed per call |
| Which rows fit, for a renderer that only has `&self` | `guitk::scroll_window::visible_count` / `Rows` — truncates rather than drawing a row across the bottom edge, and clamps a stale offset to the last page instead of going blank |
| Wheel notches → row steps, accumulating fractions so a trackpad is not dead | `guitk::wheel::Accumulator::rows` |

So the work is to hold a `ListViewport` on `ExplorerState`, drive the three
layout paths from `visible_range`, and route `Scroll { dy }` through an
`Accumulator`. **Explorer's own `move_selection` should go away in the process**
— it is a hand-rolled `select_prev`/`select_next` with its own first-press rule,
written before this entry noticed the toolkit had one, and two implementations
of "move a selection down a list, clamped" is the glob-matcher mistake
(design-decisions.md → 555) in miniature.

**The one genuine mismatch.** `ListViewport::selected` is `Option<usize>` — one
picked row — while explorer holds a multi-selection (`Ctrl+A` selects
everything). The viewport half applies unchanged; what needs deciding is whether
`ListViewport` grows an anchor/extend notion or whether explorer keeps its
`Vec<usize>` and uses the viewport purely for scrolling, syncing the "cursor"
row into it. The second is smaller and probably right, since a multi-select
anchor is a file-manager concern rather than a settings-panel one. The icon and
column views also need the offset expressed in their own units (rows of icons,
not rows of files).

**How you would notice.** Open a directory with a hundred files. You can see
about twenty. Press Down thirty times: the selection highlight vanishes off the
bottom and the listing never moves.
