## C-NONOGRAM-THE-CLICK-SLOP-WAS-ALL-ON-TWO-SIDES (lane C, 2026-08-20) — FIXED

**In short:** nonogram's cells have a 2px gap between them, and the old hit test
handed the whole of each gap to the cell *before* it. That gave the right-hand
and bottom edges of the grid two pixels of forgiveness past the last painted
cell, and the left and top edges none at all — the same click, the same distance
outside the grid, worked on two sides and did nothing on the other two. Each
cell now owns half the gap on either side of it, so the slop is one pixel on
every side. Found while collapsing the grid's geometry, which had been written
out at seven sites plus an eighth copy in the tests.

### The asymmetry

```rust
let cell_step = CELL_SIZE + CELL_GAP;
let col_f = (mx - grid_origin_x) / cell_step;
if col_f >= 0.0 { let col = col_f as usize; if col < self.grid_side { … } }
```

Cell *i* owned `[origin + i*step, origin + (i+1)*step)` — its own 28px plus the
2px gap that follows it. Inside the grid that is merely generous. At the
boundaries it is lopsided: a click one pixel left of the first column was
rejected, and a click one pixel right of the last column was accepted. This is
the same family as `C-GOMOKU-THE-CLICK-SLOP-ONLY-WORKED-ON-TWO-EDGES`, though
the cause is different — gomoku's edges disagreed because a float-to-integer
cast truncates toward zero, nonogram's because the gap was only ever added on
one side.

`axis_index` now shifts by half a gap before the range check, so a cell owns
`[origin + i*step - gap/2, origin + i*step + CELL_SIZE + gap/2)`: forgiving
between neighbours, and exactly `CELL_GAP / 2` of slop past each of the four
outer edges. Slop is wanted here — nonogram is a game of many rapid clicks —
which is the opposite of the call sudoku made for the opposite reason
(design-decisions.md §486). `the_grid_edges_take_the_same_slop_on_all_four_sides`
is the test that pins it.

### The duplication underneath it

`CELL_SIZE + CELL_GAP` was spelled out five times (`handle_mouse_playing`,
`render_playing`, `render_col_clues`, `render_row_clues`, `render_grid`) and the
grid's origin twice. That origin is not a constant: it is pushed right and down
by however much room the current puzzle's clues need, so the hit test's copy and
the renderer's copy had to agree about `CLUE_SLOT_W` and `CLUE_SLOT_H` as well
as about the cells. `test_mouse_click_grid_fills_cell` then worked the origin out
a third time for itself, which meant the one test aimed at the hit test was
measuring its own arithmetic and would have passed with the grid painted
anywhere at all — design-decisions.md §486.

Now one `// ── Grid geometry ──` section: `CELL_STEP`, `row_clue_area_w`,
`col_clue_area_h`, `grid_origin`, `grid_pixel_span`, `cell_origin`,
`cell_center`, `axis_index`, `cell_at_point`. The clue bands are positioned by
subtracting their own width from the grid's origin rather than by restating
`PADDING + HEADER_HEIGHT`, and the window is measured from the grid's far edge,
so neither can drift away from the cells.

### Outcome

122 tests green, up from 113: nine written to §485's checklist, including two
that check a clue is drawn level with the row (or over the column) it describes,
and one that no five-cell group rule passes through a painted cell. The first
mutation sweep came back 24/25; the survivor gave `grid_pixel_span` a trailing
gap the last cell does not have, which made the window two pixels wider than it
should be — visible to nothing, because the tests only asked that the grid be
*inside* the window. Requiring the margin right of the last column to equal the
margin left of the clue band closed it: 25/25 on the re-run. rustfmt drift
233 → 0 (committed separately), binary clippy 39 → 40 (one more
`arithmetic_side_effects`, the category the whole file is already full of),
test-module warnings 37 → 0.
