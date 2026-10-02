## `C-CHECKERS-THE-BOARD-GEOMETRY-WAS-WRITTEN-A-DOZEN-TIMES` (lane C, 2026-08-20) — FIXED

**Status:** FIXED 2026-08-20

**In short:** checkers worked. Like the chess and reversi entries above, and
unlike the gomoku one, this is the duplication caught before it produced a
symptom. Where a square gets painted and which square a click lands on were
computed independently from the same arithmetic, copied out at about a dozen
sites — the `7 - row` flip that puts rank 1 at the bottom of the window
appeared three times, once of them written backwards by hand inside the hit
test. Nothing but care kept the board a player sees in the same place as the
board a click resolves against.

**Where:** `apps/checkers/src/main.rs` — `handle_mouse`, `render`'s square
loop, `render_piece`, the legal-move dots, the rank and file labels, the board
frame, the window height, the panel height and `PANEL_X`.

### What was wrong

Nothing observable, and — as in reversi — the hit test was *right*: it
checked `board_x >= 0.0 && board_y >= 0.0` **before** casting, so it did not
have the truncate-toward-zero asymmetry that bit gomoku. What it had instead
was the arithmetic itself, written out again:

```rust
let board_x = event.x - BOARD_OFFSET_X;
let board_y = event.y - BOARD_OFFSET_Y;
if board_x >= 0.0 && board_y >= 0.0
    && board_x < SQUARE_SIZE * 8.0 && board_y < SQUARE_SIZE * 8.0
{
    let col = (board_x / SQUARE_SIZE) as i8;
    // Screen y=0 is top, row 7 is at top
    let row = 7 - (board_y / SQUARE_SIZE) as i8;
    self.click_square(Pos::new(row, col));
}
```

alongside `let screen_row = 7 - row;` in the render loop and a third copy in
the rank-label loop, `BOARD_OFFSET_X + col as f32 * SQUARE_SIZE` at three
places, `+ SQUARE_SIZE / 2.0` for the centre at four — the legal-move dots,
the two label loops, and `render_piece`, which was handed a square's corner
and worked out its middle for itself — and the board's extent as a literal
`SQUARE_SIZE * 8.0` at nine. A *test* wrote the mapping a further time:
`test_app_mouse_click_on_board` computed c3's pixel coordinates by hand, flip
included, so it would have moved along with a renderer bug instead of catching
it.

### Fixed 2026-08-20

One `// ── Board geometry ──` section holding `square_origin`,
`square_center` and `square_at`, plus a `BOARD_PIXEL_SIZE` constant. The flip
is stated once, in `square_origin`, and `square_at` is the only thing that
undoes it. `render_piece` now takes the centre rather than the corner, so the
middle of a square is worked out in exactly one place. `square_at` keeps the
correct order — bounds first, cast second — with a comment saying why,
since that is the exact mistake gomoku next door made.

Guarded by nine new tests built from the checklist in `design-decisions.md`
§485: the lattice measured against the window, nine probe points across
every square, an inward-and-outward sweep at all four edges, a far sweep well
past the board, pieces and dots measured against the painted square that
contains them, the frame's four margins, and the labels — including
`the_board_is_numbered_upwards_from_rank_one`, which requires the painted "1"
to sit *below* the painted "8" and so pins draughts' numbering against
reversi's opposite convention. 116 tests green (was 107), **20 of 20**
seeded mutations caught on the first pass, binary clippy 51 → 50, test-only
warnings 15 → 0.

**Worth knowing for the next reader:** checkers numbers its ranks **1 at the
bottom**, like chess and unlike Othello — so `square_origin` has a flip in
it and `apps/reversi`'s `cell_origin`, three directories away, correctly does
not. See `design-decisions.md` §485.
