## C-A-NESTED-SUBMENU-BELOW-THE-FIRST-LEVEL-RESOLVES-THE-WRONG-ENTRIES (lane C, 2026-08-20) — FIXED 2026-08-20

**In short:** In a menu-bar dropdown, a submenu inside a submenu — `View →
Zoom → Advanced → …` — shows the wrong list of commands, usually an empty one.
Only the first level of nesting works. Found while making the dropdowns scroll;
it is not caused by that change and predates it.

`gui/toolkit/src/menubar.rs`. An open submenu is tracked by an `OpenSubmenu`
node holding a `parent_index`, and the chain of them is a singly-linked list
from the dropdown downwards. To draw or hit-test a node you need its *entries*,
which `resolve_submenu_entries(root_children, sub)` is supposed to supply. Its
entire body is:

```rust
match root_children.get(sub.parent_index) {
    Some(MenuBarEntry::SubMenu { children, .. }) => children.clone(),
    _ => Vec::new(),
}
```

`parent_index` indexes into *the entries of the level above this node*. That is
`root_children` only for the first submenu. For a node at depth 2 it indexes
into the depth-1 submenu's children, so looking it up in `root_children` reads
an unrelated entry — which is normally not a `SubMenu` at all, giving
`Vec::new()` and a submenu that draws nothing and activates nothing. If the
root happens to have a `SubMenu` at that index, it silently shows *that* menu's
commands instead.

All six call sites pass the root children, including the four chain walkers
(`click_in_submenu_chain`, `hover_in_submenu_chain`, `scroll_in_submenu_chain`,
`render_submenu_chain`) which recurse into `sub.child` while passing
`root_children` through unchanged. So every path is wrong at depth ≥ 2, not
just one of them.

The function knows. Its doc comment claims "we build the path by collecting
parent indices from the root submenu down to the target", which the body does
not do, and 45 lines of comment inside the body talk the problem in a circle —
"the only reliable approach is to walk the chain from the very first submenu
node … but we don't have the root submenu pointer" — before giving up on a
lookup that is right only at depth 1.

**The proper fix** is smaller than the comment: every walker already recurses
from the top of the chain downwards, so each level has its own entries in hand
by the time it recurses. Resolve before recursing and pass *this level's*
resolved entries as the child's parent entries, rather than passing
`root_children` down untouched. `resolve_submenu_entries` then takes "the
entries of the level above" and its body is already correct for that. The
keyboard paths, which grab the deepest node with `deepest_submenu_mut` and have
no chain to descend, need a combined `deepest_with_entries` helper that walks
down accumulating entries and returns both.

**Until then:** submenus one level deep — which is all the toolkit's own
fixtures and, as far as a grep shows, all its callers — behave correctly.

**Fixed** exactly as proposed. Each of the four walkers now resolves its own
level before descending and hands the recursion *those* entries;
`resolve_submenu_entries`'s first parameter is named `parent_entries` and
documented as the level above, which its body was always right for. The 45
lines of circular comment are gone, replaced by the one sentence that settles
it. `render_submenu_chain` became a free function in the process: as a method
it could reach `self.items` at every level, which is precisely the mistake, so
taking `&self` away from it makes the wrong version unwritable rather than
merely absent. The keyboard, which jumps to the deepest node instead of
descending, got `deepest_with_entries` (and an immutable twin) that walks and
resolves together; `find_deepest` had no callers left and was deleted.

Four regression tests, all confirmed to fail when the recursion is handed
`parent_entries` again: the depth-2 panel draws its own labels and not the
decoy's, a click there activates the id that was drawn under it, two Downs and
Enter reach the second of two deep entries, and the wheel finds a deep list
long enough to scroll. The fixture parks a *decoy submenu* at the root index
the depth-2 node's `parent_index` equals, so the old code resolved to a
plausible-looking wrong menu rather than to nothing — a test against an empty
list would have passed against half the bug.
