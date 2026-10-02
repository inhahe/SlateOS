## `C-SCROLL-DELTA-UNITS-ARE-DOCUMENTED-WRONG`

**In short:** when you turn the mouse wheel one notch, the number the system
hands the app is **1**. But the comment on that number says it is a distance in
*pixels*, so about half the apps multiply it by 20 or 40 to turn "pixels" into
something useful, and the other half ignore its size and just move three rows.
The result is that one notch of the same wheel scrolls a different amount in
every app — and in the text editor it scrolls **nothing at all**, ever.

**Where:** the declaration is `gui/toolkit/src/event.rs:75` —

```rust
/// Scroll wheel (dx, dy in pixels).
Scroll { dx: f32, dy: f32 },
```

— and the only thing in the tree that actually produces one is
`gui/compositor/src/present/host.rs:320`, `wheel_delta()`, which returns
`raw / WHEEL_DELTA` where `WHEEL_DELTA` is 120. That is **notches**: exactly
`1.0` per detent, and a fraction of one for a high-resolution trackpad. The
doc comment is simply wrong, and has been since it was written.

**What each consumer currently does with one notch (`dy == 1.0`):**

| Interpretation | Apps | One notch moves |
|---|---|---|
| `dy * 40.0` | spreadsheet | 40 px |
| `dy * 20.0` | benchmark, devicemanager, diskimager, sysinfo | 20 px |
| `dy * SCROLL_SPEED` | filediff | one `SCROLL_SPEED` |
| `dy` raw | emojipicker, remotedesktop | **1 px** — visually nothing |
| `dy / line_height * 3.0` | editor | **0 lines — the wheel is dead** (see below) |
| sign only, 3 rows | procexplorer, sysmonitor, terminal, settings | 3 rows |
| sign only, 1 row/step | netscan, imageviewer | 1 row / one zoom step |

The editor case is the worst and is worth stating exactly, because it is a
plain user-visible bug rather than an inconsistency:
`apps/editor/src/input.rs:579` computes
`let lines = (dy / self.line_height * SCROLL_LINES_PER_NOTCH) as i64;` with
`line_height = 21.0` and `SCROLL_LINES_PER_NOTCH = 3.0`. One notch gives
`1.0 / 21.0 * 3.0 = 0.143`, which `as i64` truncates to `0`, and the next line
is `if lines == 0 { return EditorResponse::Idle; }`. **You would need to turn
the wheel seven notches in one event to move a single line.** This code is the
only consumer that reads the doc comment literally, which is what makes it the
proof that the doc comment is the defect.

**The proper fix**, in this order:

1. ~~Correct the declaration in `event.rs` to say notches~~ — **done**
   (`c41dac699`). The producer was already right, so nothing in the
   compositor changed.
2. ~~Give the toolkit one shared converter~~ — **done**: `gui/toolkit/src/wheel.rs`,
   `wheel::Accumulator::rows()` for row-based views and `wheel::pixels()` for
   continuous ones, at `ROWS_PER_NOTCH = 3.0`.

   It went in its own module rather than into `scroll_window` as planned
   above, because it is **stateful and `scroll_window` is deliberately not**.
   Point 4 forces that: a converter that rounds each event on its own throws
   a trackpad's `0.1`s away and the view never moves — the editor's bug in a
   different disguise — so it has to bank the remainder in a `residue` field.
   `scroll_window`'s whole contract is that it is a pure function callable
   from `render(&self)`; giving it state would have broken that for every
   existing caller.
3. Convert the twelve consumers. **In progress:**

   | Consumer | Status |
   |---|---|
   | editor | done, `453bc70b4` — the dead wheel now moves 3 lines a notch |
   | emojipicker | done, `6912e8f38` |
   | remotedesktop | done, `ffd9d7b25` |
   | diskimager | done, `344301f1c` |
   | devicemanager | done, `676fa35b0` — sidebar in rows, properties panel in pixels |
   | sysinfo | done, `ffe8dad25` — both panes in rows; see the note below |
   | benchmark | done, `0bc50c220` — pixels, bounded by a `content_bottom`-measured `max_scroll()`; see the note below |
   | spreadsheet (`* 40.0`) | to do |
   | filediff (`* SCROLL_SPEED`) | to do |
   | procexplorer, sysmonitor, terminal, settings, netscan (sign only) | to do — correct in effect, but they should stop open-coding it and pick up trackpad support |
   | musicplayer, partmanager, credmanager | not yet examined |

4. ~~Accumulate the fraction across events rather than truncating each one~~ —
   done, and pinned by `a_trackpads_fractions_add_up_instead_of_vanishing`
   (ten `-0.1` events must total three rows) and by a 500-event drift test.

**A note for the remaining conversions, learned from the three done so far:**
every one of these apps already had a passing scroll test, and not one of them
could have failed. They were written in the units of the bug — a `dy` of
`-30.0` or `-10.0`, a pixel distance sized so the broken arithmetic would
produce a visible number — and then asserted something like `offset >= 0.0` or
`> 0.0`, which is equally true of an offset that never moved. Two further
traps, both of which bit:

- **Check the fixture can actually scroll.** emojipicker's grid cannot
  overflow its panel in *any* category tab (82 emoji over eight categories,
  the largest filling 160px of a 320px panel), and remotedesktop's sample
  history is seven rows against a pane that fits eleven. A scroll test on
  either asserts nothing. Both now assert `max_scroll() > 0` in the fixture
  itself, so it cannot silently degrade.
- **Assert a row, not a non-zero number.** `> 0.0` passes under a
  one-pixel-per-notch defect. The assertion has to be `>= row_height`.

**A third trap, from sysinfo (`ffe8dad25`): the wheel is rarely the only thing
wrong with the pane.** The grep that finds `dy * 20.0` finds one line; the
conversion turned up five more defects in the same two panes, none of which
the wheel fix would have addressed and none of which had a test:

| Also wrong | Symptom |
|---|---|
| neither offset was bounded at the far end | scrolling past the last row kept the number climbing while the list stood still, and the same distance had to be scrolled back before anything moved |
| the click and the hover each derived "which row is under the pointer" separately, and **neither subtracted the scroll offset** | after scrolling, clicking a row selected a different one, and the highlight followed the click rather than the pointer |
| the hit test was not ranged to the pane | empty space below a short list hit-tested as a row |
| PageDown had no bound, and stepped a fixed row count rather than the screenful showing | paging past the end ran up the same invisible debt as the wheel |
| keyboard navigation never touched the scroll offset | arrowing past the last drawn row selected a category off screen, and the wheel was the only thing that could reveal which |
| the row stripes were keyed on the screen slot, not the row index | the whole table's colouring inverted whenever it scrolled by an odd number of rows |

The pattern to carry into the remaining conversions: **a pane whose wheel was
wrong is a pane nobody has driven.** Budget for auditing the whole pane — its
bounds, its hit test, its keyboard paths and its renderer — not just the line
the grep found. Four of the six above were found by *writing the tests*, not by
reading the code.

Two of them are the divergence class in
`C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY` below: sysinfo
recomputed the pane rectangle and the property table's top edge from the same
four constants at the renderer, the click handler *and* the hover handler.
They are derived once now (`pane_top`/`property_rows_top`), with a
`debug_assert!` in the renderer holding it to the same stack of furniture the
scroll bound is computed from — an invariant a comment cannot enforce.

**And a trap in the tests themselves.** The helper that read back "which rows
did the renderer draw" filtered fill-rects by the row-stripe colour, and
`COLOR_ROW_EVEN` happens to be the same RGB as `COLOR_SURFACE0`, the pane's
own background — so it reported one row more than the table drew and made two
*correct* page-step assertions fail by one. A test helper filtered on the
wrong property is as wrong as the code it is checking, and it fails in the
direction that wastes the most time: it accuses the fix.

`scripts/reintro-sysinfo.py` puts each of the nine defects back one at a time
and checks a test fails. That check is what turns "57 tests pass" into
evidence; it found nothing missing here, but only because it was written after
the tests rather than instead of them.

**A fourth trap, from benchmark (`0bc50c220`): a pixel view needs no
accumulator, and it needs a renderer that takes the offset as an argument.**

benchmark was the last `* 20.0` consumer and the first one converted to
`wheel::pixels` where the reasoning was not obvious, so both halves are worth
writing down.

*No accumulator.* The first draft gave it a `wheel::Accumulator` beside the
`wheel::pixels` call, on autopilot from the five row-based conversions before
it. That is cargo cult. An accumulator exists to bank the fraction of a notch
that would otherwise be rounded away *because a row index cannot hold it* — a
`usize` has nowhere to put 0.2 of a row. A pixel offset is an `f32` and is
already continuous: 0.2 of a notch is 14.4 px of real movement, applied
immediately. Rule: **an accumulator belongs to an integer offset, and only to
an integer offset.**

*The renderer has to take the scroll offset as a parameter.* The far-end bound
here cannot be a row count — a tab is cards, bar charts, a variable number of
sub-test rows and an optional trend graph — so `max_scroll()` measures it with
`guitk::render::content_bottom`, by rendering the tab into a scratch tree and
asking where the drawing stopped. That only works if the renderer can be asked
to draw at offset *zero*. A renderer that reads `self.scroll_y` can only ever
answer "how tall is it **from here**", which is the question whose answer you
are trying to bound; reintroducing exactly that (`render_active_tab(&mut
scratch, self.scroll_y)`) makes the offset converge on half the true limit and
is caught by three tests. Hence `render_active_tab(&self, tree, scroll: f32)`
and `render_content` passing `self.scroll_y` in at the one call site that
should.

*And the pane audit paid again.* Same lesson as sysinfo: the grep found one
line, the pane had eleven defects. The History tab's click handler skipped the
tab title (30 px) before dividing by the row height but not the summary line
or the column header (50 px more), so a click selected the row **two below**
the one under the pointer; it never checked the click was inside the content
rectangle, so the button strip below the pane selected whichever row the
arithmetic landed on; `Home` was the only scroll key, so there was no keyboard
route past the fold at all; and switching tabs kept the previous tab's offset,
opening the new one part-way down its own content. All eleven are put back one
at a time by `scripts/reintro-benchmark.py`, each caught by a named test.

*A note on that script:* it originally reported four defects as pinned by
"(build/other failure)" — no named test — which is not evidence of anything,
since a revert that fails to compile says nothing about the suite. (The real
cause was the machine's C: drive filling up mid-run.) Both reintroduction
scripts now fall back to printing the compiler's own `error` lines when a red
run names no test, so "it failed" and "it did not build" can be told apart.

**Until it is fixed:** two code comments (`apps/netscan/src/main.rs:2541` and
`apps/settings/src/main.rs:3228`) already point here to explain why they use
only the sign of `dy`. New scroll handlers should use `guitk::wheel` rather
than inventing another pixel constant.
