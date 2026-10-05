## §474 — Space *around* a row belongs to that row; space *between* cards belongs to neither

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Lists in this OS leave a few blank pixels around each row so the
rows read as separate items. Somebody clicking in that blank strip has to get
*some* answer, and until now each list answered from whatever its arithmetic
happened to do rather than from a stated rule. Two lists that look alike now
answer differently on purpose — the partition manager's disk sidebar gives the
strip to the row above it, the desktop's notification pane gives it to nobody —
so the rule is written down in each, and tested, rather than left to be inferred
from the code.

### What the two cases actually look like

| | partmanager's disk sidebar | desktop's notification pane |
|---|---|---|
| Row pitch | 48 px | card height + an 8 px gutter |
| Painted | 46 px tall, inset 4 px from each side of the column | the card, with the gutter left blank |
| Blank strip | 2 px under each row, 4 px either side | 8 px between cards |
| **Who owns the strip** | **the row above it** | **nobody** |

### The decision

**A blank strip that exists to shape a row is decoration and belongs to that
row. A blank strip that exists to separate two things is a gutter and belongs to
neither.** Which one a given strip is, is a fact about the design of that list,
not something to be derived from its arithmetic — so each list states it, in a
method the renderer calls and the hit test does not, and asserts it in a test.

### Why not one rule for both

*Option A — every strip belongs to the row above it (what the sidebar does).*
*What changes:* a click anywhere in the list hits something; the pointer never
falls between rows. Right for the sidebar, where the inset is purely cosmetic
and the rows are a single-selection list: a 4 px dead zone at the edge of a
220 px column would be a click that visibly did nothing. **Wrong for the
notification pane**, where the gutter is genuinely between two independent
cards, each with its own toggle — giving it to the card above means a click
aimed at the gap toggles an app, and the two neighbours are equally plausible
targets, so the mistake is a coin flip the user cannot predict.

*Option B — every strip belongs to nobody.* *What changes:* clicks in the gap
do nothing. Right for the pane; wrong for the sidebar, where it would put an
unresponsive 2 px band between every pair of disks and a dead margin down both
edges of the column, for no benefit — there is nothing there to click *instead*.

*Option C — each list says which, and is tested on it (chosen).* *What
changes:* nothing visible today; both lists already behaved this way. What
changes is that the behaviour is now a stated rule with a test, rather than a
side effect of whichever expression the hit test happened to contain.

### The cost

Two lists in one tree answer a superficially identical question differently,
which is a thing a reader can trip over. That is paid for directly: each side
names the other. `DiskSidebar`'s docs say the inset is "the opposite of
`notif_pane`, where the gutter between cards belongs to no card", and each has a
test whose name states its own rule
(`the_inset_and_the_gap_under_a_sidebar_row_still_belong_to_that_row`). A future
edit that swaps one rule for the other fails a test rather than shipping.

### Consequences in code

- `DiskSidebar::row_paint_rect()` is the only place the 4 px inset and 2 px gap
  appear, and only the renderer calls it. `row_at()` divides by the full pitch
  and `contains_column()` spans the full width, so neither can pick the inset up
  by accident.
- `notif_pane`'s hit test tests against the card *height* while its layout walks
  the card *pitch*, so the gutter falls outside every card's band.
- The reintroduction sweep carries a case for each direction: putting the gap
  back as a boundary in the sidebar, and handing the gutter to the card above in
  the pane.

**Revisit when:** a list needs a third answer — e.g. a drag-to-reorder list,
where the gap between two rows is a real target of its own (the insertion
point) and belongs to neither row *and* is not dead space. That is a genuinely
different rule, not a variant of these two, and would need its own method rather
than a flag on one of these.
