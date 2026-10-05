## §475 — A settings page is described once and interpreted twice: drawing and hit-testing are two readings of one walk

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the Settings app used to say where each row went in two separate
places — once in the code that draws the page, once in the code that decides
what a click landed on. Those two copies drifted, so some controls were drawn
in one place and clickable in another, and eleven controls were drawn with no
clickable region at all. Now each page is described by a single function, and
two different "readers" walk that one description: one paints, the other
answers "what is under the pointer?". A row cannot be drawn somewhere it cannot
be clicked, because one function put both of them there.

### The problem

`apps/settings/src/main.rs` had eleven `render_*_page` methods that walked a
`y` cursor down the content column, and ten `handle_*_click` methods that
reconstructed the same cursor from the same constants to work out which row a
click was in. Two independent transcriptions of one layout.

They had drifted, and the drift was invisible in the source because each copy
was individually plausible. What it cost, at the point this was written:

| Control | Symptom |
|---|---|
| Input Device (Sound) | drawn; nothing opened it — the handler's cursor stopped three rows short |
| Diagnostic data collection (Permissions) | same |
| Verbosity (Audio accessibility) | same |
| Per-app switches: Camera, Microphone, Background | drawn; only the Location list had a handler |
| Pointer size, ×5 (Interaction) | drawn; no handler at all |

Adding the three missing `show_dropdown` calls was possible, and would have
meant writing three more hand-derived `y` computations into the copy that was
already wrong — one more turn of the same crank.

### The decision

Collapse the two walks into one. A page is described by
`build_*_page<S: PageSink>(&self, s: &mut S)`, which speaks a small vocabulary
— `section`, `toggle_row`, `dropdown_row`, `slider_row`, `pill_row`,
`list_row`, `button_at`. Three sinks interpret that same call sequence:

- **`DrawSink`** paints each row as the cursor passes it.
- **`HitSink`** paints nothing and keeps the first registered rectangle
  containing the pointer, yielding a `RowHit` — a *named* control, not a
  position.
- **`AnchorSink`** paints nothing and reports where a given dropdown's closed
  button was drawn, so the popup opens against it.

The trait exposes four primitives (`x`, `y`, `advance`, `draw`, `hit_rect`);
everything else is a provided method built from those, so a row's geometry is
written down once and both readings inherit it. `handle_click` becomes
`row_at(mx, my)` followed by `apply_row_hit(hit)`, and nothing downstream of
`row_at` knows where anything was drawn.

`dropdown_layout` used to carry a per-dropdown table of anchor coordinates —
a *third* copy, and the one that decided where the list appears rather than
merely where a click lands. It now walks the page for the answer.

### What was given up

- **`impl FnOnce` in trait methods makes `PageSink` non-object-safe.** No
  `dyn PageSink`. Irrelevant here — the sinks are used generically at three
  known call sites and never stored — but it is a real constraint on the
  design's future.
- **Provided methods cannot capture `self`.** They call `self.draw(…)`, so the
  closures may only capture their own parameters. This forced `render_button`,
  `render_text_field` and `render_monitor_preview` from `&self` methods to free
  functions. Mild churn, and arguably an improvement: none of them read state.
- **Hit-testing costs a page walk.** Every click walks the page's description,
  and `dropdown_layout` walks it again. Both are a few hundred no-op calls on a
  human-timescale event; the alternative is caching the rectangles, which
  reintroduces a second copy that can go stale — the exact failure being
  removed.
- **A dropdown open on a page that does not draw it now has no layout**, so it
  is not drawn. Previously the hard-coded anchor meant it appeared anyway, in a
  place unrelated to any button. Not drawing it is the honest behaviour, and it
  is self-healing: the next click closes it.

### The related call: a row is its control's click target

Previously some pages accepted a click anywhere in the row and others required
the pointer to be inside the 44-pixel toggle itself, with no principle
distinguishing them. Now the whole row is the target, uniformly
(`ROW_HIT_WIDTH`, inset by `ROW_HIT_INSET` to match the highlight the list rows
paint). A row holds exactly one control, so there is nothing else in it for a
click to mean; and a 44-pixel switch is a needle to thread with a mouse when the
label beside it is what the user is aiming at anyway. The cost is that a click
on empty space to the right of a label now toggles the setting — deliberate,
and the same rule §474 settled for rows elsewhere.

### Alternatives considered

- **Add the three missing openers and stop.** Cheapest, and leaves eight other
  drawn-but-inert controls plus the mechanism that produced all of them.
- **A retained widget tree** — build nodes, lay them out, hit-test the tree.
  The general answer, and what a real toolkit does. Rejected here because it is
  a `guitk` change with eleven other apps' worth of consequences, whereas the
  immediate defect is one app describing itself twice. `PageSink` is
  deliberately smaller than a widget tree: no retained state, no layout pass,
  no invalidation.
- **Cache the hit rectangles at render time** and consult them on click. Halves
  the duplication rather than removing it: the cache is a second representation
  that is correct only if it was rebuilt since the last state change, and
  "clicked before the first paint" and "state changed without a repaint" both
  become live bugs.

### How it is held

`DropdownId::ALL` exists so a test can walk the enum rather than a hand-written
list; `test_every_dropdown_has_something_that_opens_it` asserts each one is
reachable from some page. `test_every_toggle_on_every_page_flips_its_own_field`
sweeps every page's switches, reading each field back through its own id, so a
toggle wired to the wrong field fails rather than surprising someone later.
The tests locate controls by asking the page for the coordinates — a test that
recomputes the layout from the same constants would pass while the shipped hit
test disagreed with the shipped renderer, which is the bug itself, one level up.
Verified by injecting a one-row drift into `HitSink`'s starting cursor: nine
tests fail.
