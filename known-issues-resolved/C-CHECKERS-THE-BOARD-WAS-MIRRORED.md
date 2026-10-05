## `C-CHECKERS-THE-BOARD-WAS-MIRRORED` (lane C, 2026-08-20) — FIXED

**In short:** a checkers board is set up with a dark square in the corner
nearest each player's left hand — the same rule as chess — and the pieces stand
on the dark squares. This one had it backwards: the bottom-left square, a1, was
light and out of play, so the whole board was a mirror image of a real one. The
game was still perfectly legal, because the playable squares of a checkers
board are a mirror image of themselves; it just did not look like a checkers
board, and the double corner (a landmark players actually name and play toward)
sat on each player's left instead of their right. The chess app shipped
alongside it, on the same 8×8 grid, had it right.

**Where:** `apps/checkers/src/main.rs` — `Pos::is_dark`, and the two loops in
`Board::new` that placed the opening position.

### What was wrong

```rust
fn is_dark(self) -> bool {
    (self.row + self.col) % 2 == 1
}
```

`Pos::new(0, 0)` is a1, so this makes a1 light. Everything downstream — which
squares can be clicked, where the twenty-four pieces start, which squares get
painted `DARK_SQUARE` — followed from it and was internally consistent, which
is why nothing caught it. The parity was also **written out three times**: once
in `is_dark` and once in each of `Board::new`'s two placement loops, which is
what would have let the pieces and the paint drift apart had anyone ever
corrected one of the three.

### Fixed 2026-08-20

`is_dark` is now `(row + col) % 2 == 0`, derived in its doc comment from the
convention rather than asserted as a magic parity, and `Board::new` asks
`is_dark()` instead of restating it. The keyboard cursor starts on a1 rather
than b1, which is where the first playable square now is.

Every board coordinate in the test module moved with it, by a mechanical
`col → 7 - col` mirror. That transform is an exact symmetry of checkers — rows,
and therefore the direction each side moves and where kings crown, are
untouched — so all 103 existing tests kept their meaning, including the three
that assert move notation (`"b3xd5"` became `"g3xe5"`). Two tests were
deliberately **excluded** from the mirror: `test_cursor_bounds` and
`test_cursor_upper_bounds` name the board's corners because they are about the
arrow-key clamp, not about any square's colour.

### Testing

Four new tests (107 green, no new clippy warnings, rustfmt clean). Nine
mutations, all nine caught. The one worth recording:

- **`the_board_is_painted_the_colour_it_says_each_square_is` reads the colour
  back off the render commands** — it finds the square drawn at
  `BOARD_OFFSET_X` with the largest `y`, i.e. the bottom-left one, and requires
  it to be `DARK_SQUARE`. That is the only test here whose expected value does
  not come from `is_dark`, and it alone caught three mutations: swapped square
  colours, a board painted one column to the right of where clicks land, and a
  board painted without its vertical flip. It asserts an `Option` rather than
  unwrapping, so "painted no bottom-left square at all" fails as a wrong colour
  rather than as a panic.
- `the_board_is_oriented_the_way_a_real_checkers_board_is` states the
  convention twice over — dark in each player's lower left, *and* the double
  corner on each player's right — so a mirrored board cannot satisfy both.

See `design-decisions.md` §482.
