## `C-REVERSI-THE-BOARD-GEOMETRY-WAS-WRITTEN-A-DOZEN-TIMES` (lane C, 2026-08-20) — FIXED

**Status:** FIXED 2026-08-20

**In short:** reversi worked. Like the chess entry above, and unlike the gomoku
one, this is the duplication caught before it produced a symptom. Where a cell
gets painted and which cell a click lands in were computed independently from
the same arithmetic, copied out at about a dozen sites, with nothing but care
keeping the board a player sees in the same place as the board a click resolves
against.

**Where:** `apps/reversi/src/main.rs` — `handle_mouse`, `render`'s cell loop,
the piece centres, the legal-move dots, the rank and file labels, the board
border, the window height, the panel height, and `PANEL_X`.

### What was wrong

Nothing observable, and notably the hit test was *right*: it checked
`bx >= 0.0 && by >= 0.0` **before** casting, so it did not have the
truncate-toward-zero asymmetry that bit chess and gomoku. What it had instead
was the arithmetic itself, written out again:

```rust
let bx = event.x - BOARD_OFFSET_X;
let by = event.y - BOARD_OFFSET_Y;
if bx >= 0.0 && by >= 0.0 {
    let col = (bx / CELL_SIZE) as i32;
    let row = (by / CELL_SIZE) as i32;
    if (0..8).contains(&col) && (0..8).contains(&row) { ... }
}
```

alongside `BOARD_OFFSET_X + col as f32 * CELL_SIZE` in the render loop,
`x + CELL_SIZE / 2.0` at five places for the centres, and the board's extent as
a literal `CELL_SIZE * 8.0` at eight. A *test* wrote the mapping a further time:
`test_app_mouse_click_on_board` computed d3's pixel coordinates by hand and then
asserted only `!app.move_history.is_empty()` — so it would have moved with a
renderer bug rather than catching it, and would not have noticed a hit test that
transposed row and column into another legal square.

### Fixed 2026-08-20

One `// ── Board geometry ──` section holding `cell_origin`, `cell_center` and
`cell_at`, plus a `BOARD_PIXEL_SIZE` constant. `cell_at` keeps the correct
order — bounds first, cast second — with a comment saying why, since that is
the exact mistake the neighbouring two apps made.

Guarded by eight new tests built from the checklist in `design-decisions.md`
§485: the lattice measured against the window, nine probe points across every
cell, an inward-and-outward sweep at all four edges, a far sweep well past the
board, pieces and dots measured against the painted cell that contains them, the
border's four margins, and the labels. 90 tests green (was 81), **17 of 17**
seeded mutations caught on the first pass, binary clippy unchanged at 40,
test-only warnings 5 → 0, rustfmt drift 26 → 0.

**Worth knowing for the next reader:** reversi's rows are numbered **1 at the
top**, which is correct — Othello's `a1` is the upper-left square, the opposite
of chess and go. `cell_origin` therefore has no flip in it, which reads like an
omission and is not. `the_board_is_lettered_and_numbered_the_othello_way`
requires the painted "1" to sit above the painted "8", so a well-meaning
"correction" fails rather than shipping. See `design-decisions.md` §485.
