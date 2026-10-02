## `C-GOMOKU-THE-CLICK-SLOP-ONLY-WORKED-ON-TWO-EDGES` (lane C, 2026-08-20) — FIXED

**Status:** FIXED 2026-08-20

**In short:** unlike chess, this one was a real bug a player could hit. In
gomoku stones sit *on* the lines, so half of a stone drawn on the boundary
hangs off the board, and clicking that overhanging half is meant to work. It
worked on the left and top edges and did nothing on the right and bottom. The
board looked symmetric and behaved asymmetrically.

**Where:** `apps/gomoku/src/main.rs` — `handle_mouse` (before the fix), now
`intersection_near`.

### What was wrong

The hit test asked its two questions in the wrong order: it computed a row and
column index, rejected the click if the index was off the board, and only then
measured whether the click was near enough to count.

```rust
let col = (mx / CELL_SIZE + 0.5) as i32;
let row = (my / CELL_SIZE + 0.5) as i32;
if col >= 0 && col < BOARD_SIZE as i32 && row >= 0 && row < BOARD_SIZE as i32 {
    // ... only now: is the click within STONE_RADIUS * STONE_RADIUS * 1.5?
}
```

The distance check is the real acceptance test and already refuses everything
genuinely off the board. The range check is a *second* edge policy layered on
it, and the two do not agree, because **a Rust float-to-integer cast truncates
toward zero rather than downward**. A click 18 px past the left edge produces
`-0.0103 as i32` = `0`, which passes `col >= 0`; the mirror-image click 18 px
past the right edge produces `15.0103 as i32` = `15`, which fails `col < 15`.
The `col >= 0` half of the guard could never fire at all.

Reproduced before fixing, with a throwaway probe test: sweeping the same
overshoot past each edge placed 2 stones off the left edge and 0 off the right.

The affected band is `reach - CELL_SIZE/2` = 18.371 − 18 = **0.371 px**, so in
practice one integer pixel column down the right and bottom edges. Narrow, but
reachable, and the width is an accident of the current constants — a larger
slop or a smaller cell turns the same code into a wide dead strip.

### Fixed 2026-08-20

One `// ── Board geometry ──` section holds `intersection` and its inverse
`intersection_near`, plus `BOARD_PIXEL_SIZE`, `LAST_INDEX` and
`CLICK_RADIUS_SQ`. The inverse **clamps into range and then measures**, so
there is a single acceptance rule applied identically on all four sides:

```rust
let col = (((x - BOARD_OFFSET_X) / CELL_SIZE).round() as i32).clamp(0, LAST_INDEX);
let row = (((y - BOARD_OFFSET_Y) / CELL_SIZE).round() as i32).clamp(0, LAST_INDEX);
let (ix, iy) = intersection(row, col);
let (dx, dy) = (x - ix, y - iy);
if dx * dx + dy * dy <= CLICK_RADIUS_SQ { Some((row, col)) } else { None }
```

The clamp cannot rescue a bad click because the distance check runs after it.
Ten further sites that had each spelled out `BOARD_OFFSET_X + col as f32 *
CELL_SIZE` — grid lines, star points, stones, the cursor, both sets of labels,
the win-line highlight, the window height, the status bar, `PANEL_X` — now read
`intersection` or `BOARD_PIXEL_SIZE`.

Guarded by a symmetry sweep that walks the overshoot from 0 to a full cell in
0.25 px steps and requires left/right and top/bottom to agree at every step —
which does not encode knowledge of this particular bug, so it also catches the
next one. 112 tests green (was 102), 15 of 15 seeded mutations caught, binary
clippy unchanged at 77, test-only warnings 12 → 0. Three of those tests exist
only because the first mutation pass found gaps; see `design-decisions.md`
§484 for what each one was blind to.

A separate `#[cfg(test)]` finding, fixed in the same change:
`test_mouse_click_too_far_from_intersection` asserted **nothing** — it ran the
click and commented that "it may or may not place. Just check no crash." It now
asserts the stone count is zero. That is `design-decisions.md` §480 turning up
again.

**Noted, not changed:** the column labels run `A`–`O` including `I`, where a Go
board skips `I`. No primary source settles whether that convention carries to
gomoku, and the app has no notation path where the choice is observable beyond
the board edge, so it was deliberately left alone rather than changed into a
second unsourced convention. See `design-decisions.md` §484.
