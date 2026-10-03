## C-FOUR-SCROLLABLE-PANES-DREW-AND-CLICKED-IN-DIFFERENT-PLACES (lane C, 2026-08-20)

**Status:** FIXED the same day, in `gui, apps: four scrollable panes drew in
one place and clicked in another`. Kept because two of these bug *shapes*
recur across `apps/` and are worth recognising on sight.
`scripts/reintro-scroll-panes.py` puts each of the thirteen resulting defects
back one at a time and checks the suite goes red for it.

Found while finishing the tree-wide `MouseEventKind::Scroll` conversion. Three
distinct faults, in four panes:

**1. A wheel handler that treated notches as pixels.** All four. The shade
multiplied by a private `30.0`; the converter's history and the notification
centre each did `offset - dy`, i.e. **one pixel per detent** against row
heights of 52 and 72 — a spin of the wheel was visually indistinguishable from
the list not moving. Fixed with `wheel::pixels(dy, row_h)`; none of the four
needed an `Accumulator`, because all four offsets are `f32` and can hold a
trackpad's fraction directly. (`Accumulator` exists only for *integer*
offsets; using it on a float offset would add latency and buy nothing.)

**2. A scroll offset clamped at one end only.** All four, again — `.max(0.0)`
stops a list going above its start and says nothing whatever about its end. In
`gui/desktop/src/notif_pane.rs` all four navigation keys were unbounded too,
and `PageUp`/`PageDown` stepped by a literal `200.0` rather than the pane's own
height, so the step was wrong on every screen but one. The structural cause is
the same one credmanager had (see the entry above): **the state did not know
how big its viewport was**, so no bound could be computed at the moment the
wheel turned. Fixed by storing the viewport size on the state and deriving
`max_scroll()` from measured content.

**3. The same layout arithmetic written more than once.** The expensive one.
`notif_pane.rs` declared `QUICK_SETTINGS_HEIGHT = 200.0` while
`render_quick_settings` walked its rows and drew **276**; both hit tests used
the constant, so every notification click and hover in the shade landed 76 px
from the pointer — a card is 80 px tall with 8 px between, so a click selected
the card *above* the one under the cursor, or fell in a gap and did nothing.
The whole existing suite passed throughout. `gui/notifications/src/main.rs` had
the same shape latent: renderer and hit test each walked the group list with
their own copy of the arithmetic, and the hit test additionally ignored the
renderer's clip, so a row scrolled *under* the panel header — drawn nowhere —
was still clickable. Fixed by making the layout a single piece of data
(`card_tops()`/`card_at()` in the shade, `center_rows()` in the daemon) that
the renderer, the hit test and the scroll bound all read.

**What to look for elsewhere.** Any app that (a) derives a row index from a
scroll offset in one function while a renderer derives a row *position* in
another, or (b) clamps a scroll offset with `.max(0.0)` and nothing else.
Un-audited candidates, all deriving an index from an offset:
`apps/compass/src/main.rs:399`, `apps/jsonviewer/src/main.rs:2513`,
`apps/logviewer/src/main.rs:1099`, `apps/regextester/src/main.rs:2073` and
`:2287`, `apps/renamer/src/main.rs:1663`, `apps/snippets/src/main.rs:1914`,
`apps/speedtest/src/main.rs:1097` and `:1127`.

**A test that only probes row centres does not pin fault 3.** The first
version of `clicking_a_card_where_it_is_drawn_selects_that_card` took its
coordinates from the same layout helper the hit test uses, so the two were
wrong together and it passed. It now reads the card positions out of the
`RenderCommand`s `render()` actually emits, and probes each rectangle's top
edge, middle, bottom edge and the gap below it —
`a_cards_hit_rectangle_is_exactly_the_rectangle_it_is_painted_in`.
