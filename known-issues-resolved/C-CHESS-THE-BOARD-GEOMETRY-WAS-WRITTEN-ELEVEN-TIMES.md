## `C-CHESS-THE-BOARD-GEOMETRY-WAS-WRITTEN-ELEVEN-TIMES` (lane C, 2026-08-20) — FIXED

**In short:** chess worked. This is the same duplication that made a mirrored
board possible in checkers and dead gaps possible in the emoji picker, caught
before it produced a symptom rather than after. Where a square gets painted and
which square a click lands on were computed independently, from the same
arithmetic copied out eleven times, with nothing but care keeping the board a
player sees in the same place as the board a click resolves against.

**Where:** `apps/chess/src/main.rs` — `handle_mouse`, `render_board`, the rank
and file labels, the legal-move dots and capture rings, the window height, and
`PANEL_X`.

### What was wrong

Nothing observable. That is the point of the entry: the eleven copies agreed on
2026-08-20, and no mechanism made them agree on 2026-08-21. Specifically, the
square's top-left corner was spelled out five times:

```rust
let screen_row = 7 - row;
let sx = BOARD_OFFSET_X + col as f32 * SQUARE_SIZE;
let sy = BOARD_OFFSET_Y + screen_row as f32 * SQUARE_SIZE;
```

the board's far edge four times as `SQUARE_SIZE * 8.0`, and the hit test
inverted the whole thing by hand in `handle_mouse`. A *test* wrote the mapping
a fourth time — `test_mouse_click_on_board` computed e2's pixel coordinates in
a comment and then again in code — so it would have moved with a renderer bug
rather than catching it.

### Fixed 2026-08-20

One `// ── Board geometry ──` section holding `square_origin`, `square_center`
and `square_at`, plus a `BOARD_SIZE` constant. Every one of the eleven sites now
reads from it. `square_at` keeps its bounds check **before** the cast, with a
comment saying why: a float-to-integer cast in Rust saturates, so without the
guard a click 30px left of the board would land on the a-file instead of on
nothing. That guard existed before and was correct; the comment is new, because
a guard whose purpose is not written down is a guard someone deletes as
redundant.

### Testing

Eight new tests (121 green, up from 113; fmt clean; binary clippy warnings fell
78 → 76 as the duplicated arithmetic went away, and the test module gained the
`#[cfg(test)]` lint allow `CLAUDE.md` prescribes, clearing its last three).
Fifteen mutations, all fifteen caught.

Per §481, collapsing paint and hit test into one function *removes* the
disagreement a click test was looking for, so the load-bearing assertions are
the ones sourced from outside the mapping:

- **`the_painted_board_is_sixty_four_squares_on_an_even_grid`** measures the
  paint against the window and against itself — 64 fills, eight distinct
  columns and eight distinct rows, evenly spaced, starting inside the window
  and stopping short of the side panel. No call to `square_origin`.
- **`white_is_painted_at_the_bottom_of_the_window`** is the only check that can
  see the row flip at all. Painting and hit-testing both flip, so a round-trip
  stays consistent either way; this one asserts that the white king's glyph is
  drawn lower down the window than the black king's, which is a fact about
  chess rather than about this code.
- **`nothing_outside_the_board_lands_on_a_square`** sweeps to 200px *past* the
  far edge, because the faults it hunts — saturation and wraparound — live
  beyond the board rather than just outside it.

**One mutation initially survived, and it is the finding worth keeping.** Making
`square_center` return the square's *corner* passed the whole suite: the dot
test clicked each dot and checked which square came back, and a corner belongs
to the same square its centre does. An inverse maps every point in a cell to
one answer, so round-tripping through it is blind to position within the cell by
construction. The dot test now finds the painted square that *contains* the dot
and requires the dot to be at that rectangle's centre — an expected value taken
from a second, independently emitted render command rather than from the
function under test. See `design-decisions.md` §483.
