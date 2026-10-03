## `C-KANBAN-CARD-DETAIL-PANEL-HAS-A-DEAD-SCROLL-OFFSET` — **fixed, `90a1de9b0`**

**In short:** the sixteenth instance of the dead-scroll-offset class above, left
unfixed because it is a different shape and the fix is not the same one. The
kanban card-detail panel has a scroll position that nothing reads, so a card
with a long description, a long checklist and several comments draws past the
bottom of the panel with no way to reach the rest.

**Fixed, but not the way this entry predicted** — see "What the fix actually
turned out to be" at the end.

**Where:** `apps/kanban/src/main.rs` — `detail_scroll: f32` declared at line
918, initialised at 952, and mentioned nowhere else (the two-hit grep
signature). The panel is `render_card_detail` at line 1860.

**Why it was not fixed with the others.** The fifteen fixed above are all
**row lists**: uniform-height rows, so "which rows fit" is a division, and
`scroll_window::visible` answers it. The card detail panel is free-flowing
content — a wrapped description, then a checklist, then a comment thread —
where each item's height depends on its text. `scroll_window::visible_variable`
exists for that (kanban's own board columns use it), but it needs a measured
height per item, and the detail panel currently computes its heights *while*
drawing rather than up front. Doing this properly means the same split kanban
already made for cards: a `detail_item_height()` that measures, used by both
the layout pass and the renderer, so the two cannot disagree.

**The proper fix:** measure the panel's content into a `Vec<f32>` of item
heights the way `card_height` already does for the board, window it with
`scroll_window::visible_variable`, make `detail_scroll` a `usize` item index
(not an `f32` — the panel scrolls by whole items), and reserve the "N more"
footer unconditionally per design-decisions.md §470. kanban does have an event
handler, so unlike several of the fifteen this fix would be reachable as soon
as apps are hosted at all — see `C-NO-APP-IS-WIRED-TO-AN-EVENT-LOOP`.

### What the fix actually turned out to be

The proposal above was wrong in its central choice, and the reason is worth
keeping, because the same choice comes up for every free-form panel.

**It was worse than described.** The panel had no `PushClip` either, so the
overflow was not merely unreachable — it was *drawn over the desktop*, below the
modal it was supposed to be inside. The dead-offset grep finds the missing
scroll; it says nothing about a missing clip, and the two travel together
because a panel nobody could scroll is a panel nobody looked at.

**`detail_scroll` stayed an `f32`.** The proposal wanted a `usize` item index on
the grounds that "the panel scrolls by whole items". It does not, and cannot: an
item here is a whole comment thread entry or a wrapped description, several
hundred pixels tall. Snapping to those would mean one keypress jumping most of a
screen, and a description taller than the modal would have no reachable middle
at all — there is no index that names a position part way down one item. An
index is right for kanban's *board columns*, whose items are cards of a bounded
height; it is wrong here. **The unit follows the item size, not the container.**

**The `Vec<f32>` of measured heights was not needed, and building it would have
been the mistake.** A `detail_item_height()` used by "both the layout pass and
the renderer" is exactly two derivations of one layout — the thing
`C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY` is about. They agree
the day they are written and drift the first time someone adds a line to one.
What the panel actually needed was `guitk::render::content_bottom`: render the
body into a `RenderTree` that is thrown away, and measure *that*. It is the
first derivation's output, so there is nothing for it to disagree with, and it
needs no per-item bookkeeping at all.

**Generalisation:** a panel whose content is a list of same-shaped rows wants
`scroll_window` and an index. A panel whose content is *prose and furniture*
wants pixels and `content_bottom`. Reaching for the row machinery because the
last fifteen fixes used it is how a free-form panel ends up with an item index
it cannot honour.
