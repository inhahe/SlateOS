## §473 — A collapsed panel keeps its scroll position, measured against the size it has when open

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The partition manager has a "Pending Operations" panel at the
bottom of the window that you can shut by clicking its title bar. Shut, it is
28 pixels tall — the title bar and nothing else. The list inside it can be
scrolled, and the question is what happens to that scroll position while the
panel is closed. The bug that raised the question: the scroll limit was worked
out from *how tall the panel is right now*, so while it was shut the limit was
computed against a zero-pixel-tall window and came out as "the entire list."
The wheel also worked over the shut panel's title bar, so a stray scroll there
would park the closed panel below its own last row — which you only discovered
when you opened it again and found it blank.

**Decision: the scroll limit is always measured against the panel's *open*
height, and shutting the panel neither moves the list nor forgets where it
was.** Separately, the wheel no longer answers over the title bar at all, so a
shut panel cannot be scrolled in the first place.

| Option | *What changes* | Why not / why |
|---|---|---|
| Limit measured against the current height | While shut, any scroll position is legal | This *is* the bug. A zero-tall viewport makes "scrolled to the end" mean "the whole list is above the fold," and the panel reopens showing nothing. |
| Reset the scroll to the top when the panel is shut | Reopening always shows operation 1 | Loses the user's place for no benefit. Shutting a panel to get screen room and reopening it thirty seconds later should not lose your position in a forty-item queue. |
| **Limit measured against the open height (chosen)** | Reopening shows exactly what was showing when you shut it | The scroll offset becomes a property of the *list*, not of whether it happens to be visible, which is what it already was everywhere else. |

**The cost, stated plainly:** the geometry type's `max_scroll` takes no `&self`
— it deliberately ignores the panel's measured height in favour of a constant.
That reads as an oversight, so it carries a comment saying it is not one. The
alternative, threading an "as if open" flag through the geometry, buys nothing
and gives a future reader two heights to choose between, which is the exact
mistake the type was introduced to end.

**Consequences in code** (`apps/partmanager/src/main.rs`):

- `QueueList::max_scroll(count)` is an associated function over
  `QUEUE_PANEL_HEIGHT - QUEUE_HEADER_HEIGHT`, not a method over
  `self.viewport_height()`.
- `QueueList::contains` is empty by construction while the panel is collapsed
  (`data_top == bottom`), so the wheel and the hover both refuse a shut panel
  without either of them separately asking whether it is open. Only
  `contains_header` answers there, and it toggles.
- `clamp_queue_scroll()` runs on every path that shortens the queue — undo,
  clear, apply — because a scroll limit that is never re-checked is a limit
  only at the moment it was set.

**Revisit when:** the panel becomes resizable by dragging its title bar. Then
the "open height" stops being a constant and the scroll limit has to be
measured against the last height the user chose, which is a stored value rather
than a compile-time one. The shape of the decision is unchanged — it is still
"the size it has when open" — but `max_scroll` would then legitimately take
`&self`.
