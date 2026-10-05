## C-SUDOKU-THE-GRID-GEOMETRY-WAS-WRITTEN-FIVE-TIMES (lane C, 2026-08-20) — FIXED

**In short:** where sudoku painted a cell and which cell it thought you had
clicked were worked out by different code, and the arithmetic that turns a row
or column number into a pixel offset was written out three separate times, with
a fourth statement of the grid's total width and a fifth, dead wrapper on top.
Nothing was visibly wrong — this is the preventive case again — but the tests
that were supposed to be watching had been aimed at the wrong function, so
nothing would have caught it if it had been.

### What was there

Five functions, all describing one grid:

```rust
fn grid_total_size() -> f32 {
    // 9 cells + 6 inner cell gaps (within boxes) + 2 box gaps
    CELL_SIZE * 9.0 + CELL_GAP * 6.0 + BOX_GAP * 2.0
}
fn cell_pixel_pos(row: usize, col: usize) -> (f32, f32) { … }      // renderer
fn inner_gaps_before(pos: usize) -> f32 { … }
/// Simplified cell_pixel_pos using the inner_gaps_before helper properly.
fn cell_pixel_pos_clean(row: usize, col: usize) -> (f32, f32) { … } // tests
fn pixel_to_grid_coord(pixel: f32) -> Option<usize> {
    for i in 0..GRID_SIZE {
        let pos = i as f32 * CELL_SIZE                              // …and again
            + inner_gaps_before(i) * CELL_GAP
            + (i / BOX_SIZE) as f32 * BOX_GAP;
        …
    }
}
// Override cell_pixel_pos to use the clean version
fn cell_pos(row: usize, col: usize) -> (f32, f32) { cell_pixel_pos_clean(row, col) }
```

`cell_pos` had no callers at all. `grid_total_size` restated the spacing rule in
its own words, so a change to it would have left the frame and the cells
disagreeing about how big the grid is. And the two that mattered were the twins:
`render_grid` called `cell_pixel_pos` at both its cell loop and its box-border
loop, while **every geometry test called `cell_pixel_pos_clean`** — so the suite
was green about a function the app never ran. See design-decisions.md §486.

The five pre-existing geometry tests were each an instance of an anti-pattern
already named: `test_grid_total_size` restated the function's own formula as its
expected value (§481), `test_pixel_to_grid_round_trip` proved membership rather
than placement (§483), and the rest probed one point of one cell (§480).

### What it is now

One `// ── Grid geometry ──` section: `axis_offset` (the one place the cell/gap
arithmetic is written), `grid_total_size` and `box_pixel_span` derived from it,
`cell_origin`, `cell_center`, `axis_index` and `cell_at`. `handle_mouse` went
from twelve lines to three. `axis_index` performs no float-to-integer cast at
all, so gomoku's truncate-toward-zero hazard
(`C-GOMOKU-THE-CLICK-SLOP-ONLY-WORKED-ON-TWO-EDGES`) cannot recur here.

Nine tests written to §485's checklist replace the five. The mutation sweep came
back 20 of 22 on the first pass; both survivors were mutations of `cell_center`,
which nothing outside the tests called — the renderer had spelled the digit's
centring out for itself as `px + CELL_SIZE / 2.0 - 7.0`. Making the renderer
call `cell_center`, naming the two literals `DIGIT_HALF_WIDTH` /
`DIGIT_HALF_HEIGHT`, and having
`a_digit_is_painted_inside_the_cell_that_holds_it` require the glyph to sit in
the middle half of its cell rather than merely inside it closed both.

Final: 106 tests green (102 before, net of the five deletions), 23 of 23
mutations caught, rustfmt drift 368 → 0 (committed separately as `ddfea9465`),
binary clippy unchanged at 95, test-module warnings 0.

One deliberate behaviour is worth knowing about: a click landing in the painted
line between two cells now selects neither, rather than rounding to the nearer.
The reasoning and the alternatives are in design-decisions.md §486, and
`a_click_on_the_line_between_two_cells_selects_neither` is the test that will
fail if the policy is ever changed.
