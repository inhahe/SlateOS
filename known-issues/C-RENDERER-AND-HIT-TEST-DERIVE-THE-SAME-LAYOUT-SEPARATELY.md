## `C-RENDERER-AND-HIT-TEST-DERIVE-THE-SAME-LAYOUT-SEPARATELY`

**In short:** in an app where the list on screen is not a plain run of
equal-height rows — say it has group headings in it, or the order it is drawn
in is not the order it is stored in — the code that *draws* the list and the
code that works out *what you clicked on* are usually two separate pieces of
arithmetic that were written to agree and do not. You click one row and a
different row lights up. Found and fixed in remotedesktop and diskimager; the
sweep for it is not finished.

**The instance found (`apps/remotedesktop`, fixed in `ffd9d7b25`):** the
connections sidebar draws a 22px heading for each group followed by the 52px
profile rows in that group, with the groups sorted alphabetically. The
hit-test was

```rust
let idx = (y / SIDEBAR_ITEM_HEIGHT) as usize;   // 52.0
if idx < self.profiles.len() { self.selected_profile = Some(idx); }
```

which is wrong in two independent ways at once, and both matter:

1. **It does not know the headings are there.** They occupy 22px each, so
   every row below the first heading is offset by a multiple of 22 that the
   division never accounts for.
2. **It indexes the wrong sequence.** The sidebar is ordered by sorted group;
   `profiles` is in insertion order. Even with uniform row heights those are
   different lists.

On the shipped sample data — `[Dev/Development, Prod/Production,
Build/Development, Staging/Staging]` — a click on "Build Machine" selected
"Production DB", a different profile in a different group.

**The fix that generalises:** make the layout a *value* that both sides read,
rather than a calculation each side performs. remotedesktop now has

```rust
pub enum SidebarRow { Header(String), Profile(usize) }
pub fn sidebar_rows(&self) -> Vec<SidebarRow>   // the layout, in draw order
```

and the renderer and the hit-test both walk it. They cannot disagree, because
there is nothing left for them to disagree about. The same shape handles
scrolling for free: `scroll_window::visible_variable` takes the row heights,
so the hit-test walks the rows *starting at the scroll offset* and is
automatically right for a scrolled list too — which the old code also got
wrong.

Two behaviours fall out of the change and are worth keeping deliberately:
a click on a heading now selects **nothing** rather than sliding to whichever
row is adjacent, and a click below the last row selects nothing rather than
being clamped onto the last row.

**Where to look for more of it.** The signature is a divide-by-a-row-height in
a click handler:

```bash
grep -rn "as usize" --include=main.rs apps/ | grep -i "y /\|_y /"
```

Any hit that lands in a list which (a) interleaves headings or separators,
(b) is drawn in a different order from the collection it indexes, (c) has
variable-height rows, or (d) can be scrolled while the hit-test ignores the
offset, is the same bug. Case (d) is the easiest to check and probably the
most common, since the dead-scroll-offset sweep found so many lists whose
offset nothing read.

**A note on why this survived:** remotedesktop had 100 passing tests and none
of them clicked a profile. Render tests assert the *picture* is right and
event tests assert the *state machine* is right; nothing asserted the two
agree about geometry. The regression test that now covers it pins the exact
pixel layout in a doc comment and then clicks four y-coordinates.

**A second instance (`apps/diskimager`, fixed in `344301f1c`)** — the milder
form, and the one that shows the class is not confined to lists with headings
in them. The drive sidebar's 64px row height was written out as a bare `64.0`
in the renderer *and* as a bare `64.0` in the hit-test, and the two happened to
agree. The bug they were one edit away from was already half-present: the
hit-test added the scroll offset as **pixels before dividing**, which is case
(d) above, so any list scrolled to a position that was not a whole multiple of
64 selected the row above or below the one drawn under the pointer. The browse
tab's ISO file tree was worse — it had no hit-test at all, so the panel's stack
of furniture heights (`28`, `90`, `22`, `20`) existed in exactly one place and
there was nothing for a click to disagree with, because nothing could be
clicked.

**A test that catches this class without knowing the layout.** Rather than
pinning pixel coordinates (which remotedesktop's test does, and which has to be
rewritten whenever the design changes), diskimager compares the two derivations
against each other: render the app, find the list's `PushClip` command, and
assert its rectangle equals the one the hit-test measures.

```rust
let want_y = app.content_top() + app.iso_list_offset();
let want_h = app.iso_list_height();
assert!(rt.commands.iter().any(|c| matches!(c,
    RenderCommand::PushClip { y, height, .. }
        if (y - want_y).abs() < 0.5 && (height - want_h).abs() < 0.5)));
```

That fails the moment the drawn list and the clickable list describe different
rectangles, whatever the reason, and it needs no updating when the numbers
change. It is worth adding to every app with a clipped, clickable list.

**And the companion check, per list rather than per field:** does the draw loop
consult a scroll offset at all? `a_scrolled_file_tree_draws_the_rows_the_offset_selects`
scrolls, renders, and asserts the *first row drawn* is the one at the offset
and that the loop stops at the bottom of the pane. A renderer that ignores its
offset passes every render test ever written for it, because the picture it
draws is a perfectly valid picture of the top of the list.

**A third instance (`apps/devicemanager`, fixed in `676fa35b0`)** — three
derivations, not two, and the reason it matters is that they *agreed*.

The sidebar's rule for which tree nodes are on screen (a node is drawn if it is
`visible`, and, if it is a child, if its category is expanded) existed in the
renderer's forward walk, in `tree_hit_test`'s own loop, and a third time in
`is_node_visible` as a **backwards scan** — walk up from the node looking for a
depth-0 parent, and ask whether that parent is expanded. The backwards scan
never consults the parent's own `visible` flag, so it and the forward walk
disagree about a device whose category the search filter has hidden.

That state is unreachable today, but only because of an invariant kept in a
*fourth* function: `apply_search_filter` marks a category invisible exactly when
it has hidden all of that category's devices. Nothing ties the three walkers to
that function. Making a category match on its own name — an obvious
improvement, and one a future session would make without ever reading
`is_node_visible` — breaks the invariant and the arrow keys start selecting rows
that are not on screen.

**So: an invariant maintained somewhere else is not a reason for two
derivations to be allowed to exist.** It is a reason the bug has not fired yet.
All three now call `visible_tree_indices()`, and the regression test
(`is_node_visible_agrees_even_where_the_filters_invariant_does_not_hold`)
hand-builds the state the filter cannot currently produce, because a test that
only exercises reachable states cannot pin a rule about unreachable ones.

Worth noting how this was caught: not by the 133 passing tests, but by
deliberately reintroducing each of five defects one at a time and checking a
test failed. Four fired. The fifth — reverting `is_node_visible` to the
backwards scan — fired **nothing**, which is what exposed that the divergence
was latent rather than live, and that the doc comment claiming otherwise was
overstated.

### Measuring a panel that is not a list of rows

devicemanager's properties panel could not be given a scroll bound at all,
because its height depends on which tab is open, how many properties the
selected device has, whether a driver badge is present, and how many separators
that adds up to. There is no row count to multiply.

The obvious answer — a `measure_general_tab` beside `render_general_tab`, and so
on — is this whole issue in another costume: two derivations of one layout,
written to agree, drifting the first time someone adds a line to one of them.

The answer that cannot drift is `guitk::render::content_bottom` (added in
`44c3f6578`): render the panel into a throwaway `Vec<RenderCommand>` and measure
*that*. It is not a second derivation, it is the first one's output, so there is
nothing for it to disagree with. Three details in it are worth keeping in mind
if it is reused:

- It **ignores `PushClip` on purpose.** Honouring the clip returns the clip
  height every time, which is exactly the useless answer — the reason to measure
  is to find the content the clip is hiding.
- `RenderCommand::Text.y` is the **top** edge, not the baseline (the compositor
  adds the ascent at `gui/compositor/src/lib.rs:2764`), so a text bottom is
  `y + line_height`, and the line height depends on the font family in force —
  which means `PushFont` has to be tracked.
- A `Line`'s bottom includes half its stroke width and a `BoxShadow`'s includes
  its offset, blur and spread; a measurement that used the box alone would let a
  shadow hang off the end of the scroll range.

This is the tool for the remaining free-form panels, starting with
`C-KANBAN-CARD-DETAIL-PANEL-HAS-A-DEAD-SCROLL-OFFSET`.
