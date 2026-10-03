## TD-THE-TOP-BORDER-IS-DRAWN-OUTSIDE-THE-FRAME-INSETS (lane C, 2026-08-17) -- **FIXED 2026-09-07**

**Fixed 2026-09-07 (lane C), by the first of the two options below.**
`frame_insets` returns `top = TITLE_BAR_HEIGHT + BORDER_WIDTH`,
`title_bar_rect` starts `BORDER_WIDTH` down from the frame, and
`render_border` strokes `frame_rect` unmodified. Drawing and measurement now
name the same box.

**The second option was eliminated first, on evidence** -- see the update
below. Stroking `frame_rect` while the bar filled the whole top inset put the
outline under the title bar, which paints over it. That is why the border was
drawn outside the frame in the first place.

**The top inset is summed as two scaled values, not a scaled sum.**
`scale_dimension` rounds, so `scale(bar + border)` and
`scale(bar) + scale(border)` can differ by one, and `title_bar_rect` subtracts
the border back out to get the bar's own height. Summing the scaled parts makes
that subtraction exact at every scale factor -- a 2x display gets a 2x bar and
a 2x border, and the bar is still exactly `TITLE_BAR_HEIGHT * 2`.

**The drag boundary moved, as the entry warned, and now has a test.**
`the_border_row_resizes_and_the_title_bar_below_it_moves` presses the border
row and asserts the window resizes, then presses one row lower and asserts it
moves without resizing. Both halves, because either alone passes if the whole
top of the window does one thing.

Six existing tests pinned the old geometry and were updated with the reason
rather than the number: the frame is one row taller at the top, the bottom and
both sides are unchanged. Mutation-checked -- putting the title bar back at
the frame's top edge fails six.

**In short:** the 1-pixel line the compositor draws around a window is drawn
one pixel higher than the space the layout reserved for it. Nobody sees a
problem, because the row it lands on is part of the window's drop shadow and
is repainted anyway. But it means the code that *draws* the frame and the code
that *measures* the frame disagree by one pixel, and the next person to trust
the measurement will be wrong by one pixel too.

`Window::frame_insets` returns `(top, side, bottom) = (TITLE_BAR_HEIGHT,
BORDER_WIDTH, BORDER_WIDTH)`: a border down each side and along the bottom,
and **no border above the title bar**. Every measurement derives from that —
`frame_rect`, `outer_rect`, `title_bar_rect`, hit testing, damage tracking.

`Compositor::render_border` (gui/compositor/src/lib.rs) does not. It strokes a
box whose top edge is `BORDER_WIDTH` above `frame_rect`, so the frame is drawn
one row taller than it is measured. That row falls inside `outer_rect` (which
adds `SHADOW_SIZE` = 8 px of shadow beyond the frame) and inside
`window_drawn_extent`, so it is repainted correctly and is inside the resize
grab band — hence no visible symptom today.

**Where:** `render_border` carries the discrepancy explicitly now, as a
`Rect::new` that adds the row to `frame_rect` with a comment, rather than as
open-coded constants that merely happened not to match. `frame_insets` is at
`gui/compositor/src/lib.rs`; `nothing_a_window_draws_falls_outside_its_damage_extent`
pins the containment that keeps it harmless.

### Update 2026-09-07: one of the two options below is eliminated, on evidence

**Stroking `frame_rect` unmodified does not work.** I tried it, and the top
edge of the outline disappears: the title bar is painted *over* the frame's
first row, so a border drawn there is covered. Measured rather than reasoned --
the pixel at the frame's top-left corner reads `FF313244`, which is exactly
`theme.title_bar_focused`, where the border colour would be `FF585B70`. The
existing test `the_border_rounds_with_the_frame_it_traces` fails on the
"a square border did not paint its own corner pixel" assertion.

So the one-row-above draw is not a slip: it is compensating for the title bar
covering the row the outline would otherwise occupy. That makes the second
option below wrong and leaves the first as the fix -- `frame_insets.top`
becomes `TITLE_BAR_HEIGHT + BORDER_WIDTH` and `title_bar_rect` starts
`BORDER_WIDTH` down, with the drag tests reviewed because the boundary between
"title bar, drag to move" and "top border, drag to resize" moves with it.

The attempt was reverted rather than shipped: a window that measures correctly
and has no visible top edge is worse than the one-pixel disagreement, which
nobody can see.

**Proper fix:** decide which is right and make both agree.
- If the border above the title bar is wanted (it is what a real window frame
  looks like), `frame_insets` should return `top = TITLE_BAR_HEIGHT +
  BORDER_WIDTH` and `title_bar_rect` should start `BORDER_WIDTH` below the top
  of the frame box. This shifts every framed window's title bar and the resize
  grab bands by one pixel, and moves the boundary between "title bar" (drag to
  move) and "top border" (drag to resize) — so it wants a look at the drag
  tests, whose grab points are chosen relative to `TITLE_BAR_HEIGHT`.
- If it is not wanted, `render_border` should stroke `frame_rect` unmodified
  and the extra row disappears.

Not urgent: nothing is visibly wrong and nothing is unsafe. Logged because a
one-pixel disagreement between drawing and measurement is exactly the kind of
thing that becomes a real bug the moment either side is touched.
