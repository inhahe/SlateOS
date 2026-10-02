## C-A-SUBMENU-NEAR-THE-RIGHT-EDGE-IS-DRAWN-OFF-THE-SCREEN (lane C, 2026-08-20) — FIXED 2026-08-20

**In short:** A dropdown submenu always opens to the right of its parent panel.
For a menu near the right edge of the display the child is drawn past the edge,
where its rows can be neither seen nor clicked. The horizontal twin of the
vertical overflow fixed in "Neither menu scrolled" above.

`gui/toolkit/src/menubar.rs`. `ContextMenu::show` flips horizontally when a
popup would overflow the right edge; the menu bar's submenus never do. The
child's x is decided in three places — `MenuBar::submenu_at`, the `OpenChild`
arm of `click_in_submenu_chain`, and the hover path — and all three say
`panel.right()` unconditionally.

**The proper fix:** one `DropdownPanel::child_origin(child_width, row_top)`
that all three go through, returning `self.right()` normally and
`(self.x - child_width).max(0.0)` when the former would run past
`DEFAULT_VIEWPORT_WIDTH` — which the file will need to gain, having only
`DEFAULT_VIEWPORT_HEIGHT` today. Three places deciding one thing is the same
duplication shape the `DropdownPanel` collapse removed everywhere else in the
file, so the fix is the collapse rather than three edits.

**Fixed** as the collapse. `DropdownPanel::child_origin(child_width)` is the
one spelling of the rule and all three sites go through it;
`DEFAULT_VIEWPORT_WIDTH` joins its vertical twin. It takes only the width, not
the `row_top` guessed at above — the row's top is the child's `y`, which is a
separate question already answered by `row_top`, and folding two answers into
one function is the shape being removed rather than a smaller version of it.
The flipped position is floored at zero: a submenu wider than everything to its
left has nowhere good to go, and clipped-on-the-right is at least reachable
where a negative `x` is neither visible nor clickable.

Two regression tests, both confirmed to fail when `child_origin` goes back to
`self.right()`: a table of the rule's four cases (fits, exactly fits, one pixel
over, no room either side), and an end-to-end through `hover_in_submenu_chain`
asserting a flipped child ends flush with its parent's left edge and inside the
screen.

### Note on the reintroduction sweep this turned up

Re-running `scripts/reintro-row-hit-tests.py` over the enlarged set reported
one of 67 defects **not pinned**: `menu.rs`'s "a menu taller than the screen is
given its full height and runs off it". Investigating it showed the anchor, not
the suite, was at fault — it patched `ContextMenu::show`'s
`let panel_height = self.panel_height();` to `content_height()`, and that is an
*equivalent mutation*. `show` uses the height only to choose the upward flip,
and for any `y` inside the viewport `(y - panel_height).max(0.0)` and
`(y - content_height).max(0.0)` are both 0, so no test could ever have told the
two apart. The cap that is load-bearing is `panel_height()` itself, which every
vertical bound in the file is measured from; removing *its* `.min()` fails five
tests. The anchor was moved there.

Worth remembering: a sweep case can be wrong in a way that looks exactly like a
missing test. The first move on a NOT PINNED line is to ask what the mutation
actually changes for a caller — not to write a test that forces the mutation to
be observable, which here would have meant asserting on a `y` outside the
screen.
