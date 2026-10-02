## §471 — The kanban board scrolls as one, not one column at a time

**Date:** 2026-08-18
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** A kanban board is a row of columns ("To Do", "Doing", "Done"),
each a vertical stack of cards. When a column has more cards than fit on
screen, the ones below the fold have to be reachable somehow. The choice was
whether scrolling moves *all* the columns together, like a single sheet of
paper, or whether each column scrolls on its own with the mouse pointer
deciding which one moves. Chose: all together, one shared position.

### The decision

`KanbanApp::scroll_offset` is a single `usize` — a card index, not a pixel
offset — and every column starts drawing at that index. Page Up/Down and the
arrow keys move it. Each column then clamps it against its *own* card count
independently, via `scroll_window::visible_variable`, so a column with four
cards sits at its last page while the one next to it with sixty is still
scrolling through the middle.

### The alternatives

| | *What changes:* |
|---|---|
| **One shared offset** (chosen) | Page Down moves every column down together; a short column stops at its end and stays there while the others keep going. |
| **One offset per column** | Page Down moves only the column the selection (or the pointer) is in; the others stay where they were. |

### Why one

- **There is nothing to point at yet.** Per-column scrolling needs a way to say
  *which* column — a pointer position, or a "current column" that scroll keys
  follow. The board view has a `selected_column`, but it is moved by the same
  Left/Right keys that move cards between columns, so a scroll bound to it
  would move the view every time the user reorganised the board. Wiring a
  pointer into it is real work with no user asking for it.
- **Columns are read across, not down.** The point of the layout is comparing
  the same horizontal band across the three columns — what is in progress
  *next to* what is done. Independent offsets break that alignment: after a few
  scrolls the columns show unrelated slices, and the row you were comparing is
  in three different places.
- **The clamp makes the ragged case harmless.** The obvious objection to a
  shared offset is that columns are different lengths, so one number cannot be
  right for all of them. Clamping per column answers that: the short column
  does not go blank, it pins to its last page and waits. That is exactly the
  §470 rule, applied three times instead of once.

### The cost, stated plainly

If one column is much longer than the others, reaching its bottom scrolls the
short columns to their ends first, where they then sit unchanging — the screen
appears half-frozen while the long column moves. This is the real downside and
it is accepted: it is legible (the columns visibly hit their ends) rather than
confusing, and it is only reached on boards lopsided enough that the columns
were not comparable anyway.

### What would change this

A pointer-position hook in the board view — the moment the app knows which
column the mouse is over, per-column scrolling becomes cheap and the "read
across" argument weakens, because the user would be steering deliberately
rather than having the alignment broken out from under them. At that point
this should be revisited rather than defended.
