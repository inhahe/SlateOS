## `C-TMUX-PANES-NEVER-RESIZE-THEIR-GRID` (lane C) — ✅ **FIXED 2026-08-20**

**In short:** every tmux pane's terminal grid is a fixed 80x24 no matter how
big or small the pane is drawn. Splitting a window four ways does not tell the
four terminals they are now a quarter the size, so a program running in one
still wraps its output at 80 columns and still thinks it has 24 rows.

`TerminalBuffer::resize(cols, rows)` exists and is correct, but **its only
callers are tests**. `Multiplexer::resize_pane` adjusts the *layout*
proportions; nothing converts a pane's pixel rectangle into a cell count and
pushes it into the buffer.

**Where:** `apps/tmux/src/main.rs` — `TerminalBuffer::resize` (~line 223),
`Multiplexer::resize_pane` (~line 1600), and the layout walk in
`Multiplexer::render` that computes each pane's `(x, y, w, h)` bounds.

**The proper fix:** the layout pass has the rectangles and the cell metrics
(`char_width()`, `char_height()`), so it can compute `(cols, rows)` per pane.
It runs inside `render(&self)`, which cannot write, so the resize belongs in a
`&mut self` pass that runs before rendering — a `relayout()` called when the
window size or the split layout changes, driving `TerminalBuffer::resize`. In a
real terminal this would also be where `TIOCSWINSZ`/`SIGWINCH` equivalents go
out to the child process.

**What it currently costs, beyond the wrapping:** the scrollback clamp
(`Pane::max_scroll_back`) measures against `buffer.rows` because key handling
runs nowhere near layout and cannot know the drawn height. The two agree
whenever the grid matches the pane — the state this issue is about restoring —
and while they do not, a pane drawn *shorter* than its grid stops that many
lines short of the very top of its scrollback. Fixing the resize fixes that
clamp for free.

### Fixed 2026-08-20

Fixed as described above, but with the emphasis on making the fault
*unrepresentable* rather than merely absent — this is the third instance in
this lane of the same family (a renderer and some other consumer each spelling
out the same layout arithmetic, then disagreeing), after
`design-decisions.md` §475 and §476. See §477 for the reasoning.

Two things now exist exactly once and are read by both the paint and the
sizing pass:

- **`pane_grid(width, height) -> (cols, rows)`** — the only conversion from a
  pane's pixel rectangle to a cell count. `render_pane` asks it instead of
  recomputing `(height - 4.0) / cell_h`, so what is drawn and what was
  written into are the same grid.
- **`Window::bounds()`** — the only layout walk. `Multiplexer::render` paints
  those rectangles and `Multiplexer::relayout` converts them into cell counts.

`relayout()` is a new `&mut self` pass that pushes each pane's grid into
`TerminalBuffer::resize` — which until now had no callers outside the tests.
It runs from the nine mutations that can change a pane's rectangle (`new`,
`new_session`, `attach`, `kill_session`, `new_window`, `split_pane`,
`close_pane`, `resize_pane`, `apply_layout`) **and** from `render` as a
backstop. Both, deliberately: the per-mutation calls are what make key
handling *between* two frames — which reads `Pane::max_scroll_back` and
`Pane::page_lines` — see a current grid, and the render-time call is what
makes a future mutation that forgets to say so cost a frame's lag instead of
a terminal that wraps in the wrong place until the next split. It is
idempotent, so having both costs nothing.

The scrollback clamp noted above is fixed for free, as predicted: the buffer's
row count now *is* the pane's drawn height.

Nine new tests (102 green, clippy unchanged at its 28 pre-existing warnings).
Mutation-tested with twelve mutations — every one is caught. Two were **not**,
on the first run, and both failures were instructive:

- *Dropping the content inset* changed no column count at 1200x800, because
  4 px is less than one 8.4 px cell. Caught now by
  `a_grid_always_fits_inside_the_pane_that_holds_it`, which sweeps pane sizes
  in 0.5 px steps rather than checking one live layout.
- *Starting the layout walk at `y = 0.0` instead of `TAB_BAR_HEIGHT`* was
  invisible because both existing checks asked `Window::bounds` where the
  panes were — the very function being mutated. Caught now by
  `no_cell_is_drawn_over_the_chrome`, whose bounds come from the layout
  constants directly. That test also catches dropping `STATUS_BAR_HEIGHT` and
  overrunning `WINDOW_WIDTH`.

The general lesson, which keeps recurring: **a test that derives its
expectation from the code under test proves only self-consistency.**
