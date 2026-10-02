## §477 — A pane's grid is its drawn rectangle, converted once

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** every terminal pane in the tmux app was a fixed 80 columns by 24
rows however large it was actually drawn, so splitting a window four ways left
four programs still wrapping their output at 80 columns with blank space beside
it. The obvious fix — call the existing `TerminalBuffer::resize` from the places
that change a layout — would have needed each of those places to work out the
cell count for itself, which is the same shape of mistake that caused §475 and
§476. It was done instead by writing the pixels-to-cells conversion once and the
layout walk once, and having both the painting and the sizing read them.

### What was wrong

`TerminalBuffer::resize(cols, rows)` existed, was correct, and **had no callers
outside the tests**. `Multiplexer::resize_pane` adjusted the layout *ratios*;
nothing anywhere turned a pane's pixel rectangle into a cell count. Meanwhile
`render_pane` worked out how many cells to draw with its own arithmetic:

```rust
let visible_rows = ((height - 4.0) / cell_h) as usize;
let visible_cols = ((width - 4.0) / cell_w) as usize;
```

so the number of cells *drawn* tracked the pane, and the number of cells the
terminal *believed in* did not. The `4.0` was a bare literal appearing in two
places that had to agree with a `+ 2.0` inset elsewhere.

### The family this belongs to

This is the third instance in this lane of one fault: **a renderer and some
other consumer each spell out the same layout arithmetic, and the region drawn
stops agreeing with the region the state believes in.**

| | The two readings | What the user saw |
|---|---|---|
| §475 | a settings page drew rows and hit-tested them separately | controls that did nothing when clicked |
| §476 | a slider's value→position mapping, written at each call site | handles drawn right, undraggable |
| §477 | a pane's rectangle → cell count, in the paint and nowhere else | output wrapped at 80 columns in a 142-column pane |

The fix is the same each time and is the decision being recorded: collapse the
duplicated arithmetic into a single value or walk that every consumer reads,
so the two cannot drift. Here that is `pane_grid(width, height)` — the only
pixel→cell conversion — and `Window::bounds()` — the only layout walk.

### Where the resize is driven from, and why in two places

A `relayout()` pass (`&mut self`) walks `Window::bounds()` and pushes each
pane's grid into its buffer. Three placements were considered:

- **Only from the nine mutations that change a rectangle** (`new`,
  `new_session`, `attach`, `kill_session`, `new_window`, `split_pane`,
  `close_pane`, `resize_pane`, `apply_layout`). Correct between frames, but one
  forgotten call site silently reintroduces the whole bug.
- **Only from `render`.** Structurally safe, but wrong between frames:
  `Pane::max_scroll_back` and `Pane::page_lines` are read by *key handling*,
  which runs between two paints. A pane split and then paged before the next
  frame would page by the old screenful.
- **A `with_active_window(f)` closure funnel** that relayouts on the way out.
  Rejected: `close_pane` needs `self.find_pane_mut` after the walk and would
  fight the borrow checker for no gain in safety.

**Both of the first two were taken.** `relayout` is idempotent — resizing a
buffer to the size it already has does nothing — so the per-mutation calls cost
nothing given the render-time one, and vice versa. The mutation calls buy
between-frame correctness; the render call means a *future* mutation that
forgets to say so costs a frame's lag rather than a terminal that wraps in the
wrong place until the next split. `render` therefore takes `&mut self`: a
pane's grid is part of what a frame decides.

**Scope: every window of the attached session, and no other session.** A
background window's program keeps running and writing, so it must not learn its
new width only when the user switches to it. A detached session has no client
and therefore no size, and skipping them also bounds the walk at 32 windows
rather than that times 64 sessions.

### One consequence worth naming

`Pane::max_scroll_back` measures against `buffer.rows` rather than the pane's
drawn height, because key handling runs nowhere near layout. That was a
documented wart while the two could differ — a pane drawn shorter than its grid
stopped that many lines short of the top of its scrollback. It is now simply
correct: the buffer's row count *is* the drawn height, asked of the one place
that stores it.

### What the tests are, and what two of them cost to find

Nine tests, mutation-checked against twelve mutations; all twelve are caught.
Two mutations survived the first run, and both survivals were the same mistake
in the tests:

- Setting the content inset to zero changed no column count at 1200x800,
  because 4 px is smaller than one 8.4 px cell. A sub-cell constant is only
  visible at the widths where it straddles a cell boundary, so the test that
  catches it (`a_grid_always_fits_inside_the_pane_that_holds_it`) sweeps sizes
  in 0.5 px steps and asserts the arithmetic, rather than checking one live
  layout.
- Starting the layout walk at `y = 0.0` instead of `TAB_BAR_HEIGHT` — i.e.
  drawing the panes over the tab bar — was invisible, because both checks that
  should have caught it asked `Window::bounds` where the panes were, and
  `Window::bounds` was the mutated function. `no_cell_is_drawn_over_the_chrome`
  takes its bounds from `TAB_BAR_HEIGHT`, `STATUS_BAR_HEIGHT` and
  `WINDOW_WIDTH` directly instead, and catches all three of tab bar, status bar
  and right edge.

**A test that derives its expectation from the code under test proves only
self-consistency.** That sentence closes §476 as well; it is written twice
because it cost real mutation-testing time to rediscover.
