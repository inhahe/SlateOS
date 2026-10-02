## §472 — The podcast sidebar scrolls as one; only its title is pinned

**Date:** 2026-08-18
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The podcast app's sidebar is a single column holding a library
menu (Search, All Episodes, Queue, Downloads, History, Statistics), then your
subscriptions, then a list of twelve categories. It was drawn straight down
the column with no limit, so on anything but a tall window the bottom of it
fell off the edge of the screen — and because it was cut by the screen edge
rather than by a scroll position, there was no way to get to what fell off.
The categories were unreachable outright. The choice was whether to pin the
fixed parts in place and scroll only the subscriptions, which is what most
apps do, or to scroll the whole column together.

**Decision: the whole column below the title scrolls together.**

The deciding fact is arithmetic, not taste. The sidebar's *fixed* content —
six library entries at 32px, twelve categories at 32px, two headings and two
dividers — is 732px tall. A 600px window cannot show it **with no
subscriptions in it at all**. Pinning is not an option that was rejected on
preference; it is an option that does not exist, because there is nothing
left to pin it against.

| Option | *What changes* | Why not |
|---|---|---|
| Pin the library entries and the categories; scroll only the subscriptions | The menu and categories stay put; the middle scrolls | The fixed parts alone overflow a 600px window, so at ordinary sizes the subscription list is allotted a *negative* height and shows nothing, while the categories still run off the bottom. It fails exactly where it is needed. |
| Give the subscriptions a minimum height and let the categories be clipped | The subscriptions always show a few rows | Restores the original bug in a smaller form: whatever is clipped is clipped by the window edge, so it is still unreachable. |
| **Scroll everything below the title (chosen)** | The whole sidebar list moves under a fixed "Podcasts" heading; every row is reachable at some scroll position | Nothing is ever unreachable at any window size, and no section can starve another. |

**The cost, stated plainly:** the library menu is no longer always on screen.
Scroll down to your 80th subscription and "All Episodes" is above the fold.
That is a real regression against the pinned-sidebar convention, and it is
accepted because the convention presupposes a window tall enough to honour
it. This mirrors §471's reasoning for the kanban board: when a layout cannot
be satisfied, one scroll that always works beats several that work only at
sizes we do not control.

**Consequences in code** (`apps/podcast/src/main.rs`):

- `SidebarRow` models the column as a flat list of `Header` / `Divider` /
  `Item` rows with per-row heights, so `scroll_window::visible_variable` can
  measure it. `sidebar_rows()` builds that list in one place — a list whose
  height is computed by one function and drawn by another drifts the moment
  either gains a row.
- The now-playing bar is drawn *over* the bottom 80px of the sidebar, so the
  sidebar's usable height ends where that bar begins, not at the window edge.
  That was a second, independent instance of the same off-the-bottom bug and
  is now covered by the bounds test at every scroll position, playing and
  stopped.

**Revisit when:** the sidebar gains its own vertical constraint independent
of the window — a resizable split, or a collapsed "compact" mode that hides
the categories behind a disclosure. Either would free enough room for the
library menu to be pinned honestly, at which point pinning it is the better
behaviour and this should be reversed rather than defended.
