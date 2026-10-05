## 833. The Cards theme is a tidied version of the old look, not a copy of it

**Date:** 2026-09-11
**Lane:** C
**Decided by:** Claude (autonomous) — flagged for the operator because it is visible, in the optional theme

**In short:** the old filled look was kept as a theme you can switch back to.
It is not pixel-identical to what shipped. The same kind of box used to be
filled with several different greys in different parts of the system, and now
one kind of box has one grey. Under the new default — outlined boxes — none of
this shows at all, because there are no fills to differ.

**The measurement that forced the choice.** Counting every draw site by what it
*is* against what it was *filled with*:

| kind | shades it was drawn in |
|---|---|
| Card | `surface0` ×174, `mantle` ×99, `surface1` ×63, `crust` ×55, `surface2` ×11 |
| Selected | `surface0` ×69, `surface1` ×43, `mantle` ×4 |
| ControlTrack | `surface0` ×19, `surface1` ×19, `surface2` ×11 |
| Sidebar | `mantle` ×30, `surface0` ×7, `crust` ×5, `surface1` ×3 |

An ordinary card was drawn in **five** different greys. A slider track was
`surface0` in the system tray and `surface1` or `surface2` in the next
application along. None of that was decided; it accumulated.

**The alternative, and why it is not tenable.** Preserving each site's original
shade means `Surface::Card` cannot decide the fill on its own — the call site
would have to pass its shade in. That is choosing a colour at the draw site,
which is the exact thing this change exists to stop, and it would make the
theme switch impossible to implement in one place.

**So the trade is stated plainly:** switching to Cards gives you the filled look
with its inconsistencies removed. A card that used to be `mantle` is now
`surface0` like every other card. I judge that an improvement rather than a
loss — it is the same inconsistency the border decision was made to escape — but
it is a visible difference in a theme that was described as "keep the old look",
so it is recorded here rather than discovered later.

**What it broke, and what that tells you.** One test: the system tray's slider
track was found by matching `surface0`, and a track is now `ControlTrack`, which
is `surface2`. The test now asks the palette what a control track looks like
rather than naming a shade — which is the right shape for such a test either
way, and would have survived this change had it been written that way first.

**Two categories the same investigation added to the "do not convert" list:**

- **Hairline rules.** A 1px-tall fill is a separator — the line under a calendar
  grid, the divider between two panes — and outlining it yields a rectangle of
  zero height. The tray's calendar rule was converted and the test that measures
  "does a six-row month fit" lost the boundary it measures against. Six such
  sites tree-wide are now left alone.
- **Data bars and structural strips**, per §829's open half and C-Q14.
