## 509. A tiling drop follows the monitor under the *pointer*, not the one under the window

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous) — lane C

**In short:** When you drag a window to the edge of a screen and let go, the
compositor tiles it. On a desktop with two monitors it has to decide *which*
monitor "the edge of the screen" meant. The window and the mouse pointer are not
always on the same one — while the window is crossing the join between the two
screens it is on both, and if you grabbed a wide window near the right-hand end
of its title bar the window trails a long way behind your mouse. The choice was
whether to answer from where the pointer is or from where the window is. It
answers from the pointer, and the highlight outline you see while dragging is
drawn from the same answer, so what you are shown is always what you get.

**The situation.** `Compositor::drop_intent` runs on every mouse-move during a
window drag. It asks `guiremote::zones::drop_at(x, y, area)` whether the pointer
has entered one of the screen-edge bands, and if so what tiling that band means
(maximize at the top, halves at the sides, quarters in the corners). It needs a
`WorkArea` to ask against. Then, on mouse-up, `finish_drag` has to actually place
the window — and it needs a work area too.

Before the per-monitor fix (`known-issues.md`
`TD-C-TILING-MEASURED-EVERY-MONITOR-AT-ONCE`) this was not a question, because
there was exactly one work area: the union of every display. Once the work area
became per-monitor, both "which monitor" and "does the release re-decide"
became real questions with different answers.

**Decision 1 — the pointer's monitor, not the window's.**

| Option | *What changes:* |
|---|---|
| **The pointer's monitor** (chosen) | dragging a window into the top band of your second monitor maximizes it on the second monitor, whatever the window is currently overlapping |
| The window's monitor (largest intersection) | the same gesture can maximize it back onto the *first* monitor — the one you just dragged it off — because the window's body has not caught up with your mouse |

The pointer wins for one reason: the edge band that fired is a band *of a
specific monitor*, and the pointer is the thing that entered it. Choosing the
window's monitor means the gesture's trigger and the gesture's effect are read
off two different objects, which is exactly how a user ends up watching a
highlight on screen two and getting a window on screen one.

The two answers coincide for the ordinary case — a move drag carries the window
with the pointer — which is precisely why this was worth pinning down with a
test rather than leaving to whichever lookup a future edit happened to reach
for. They diverge in two configurations, both reachable without trying:

- **At the seam.** A window crossing the join is on both monitors, and the one
  holding the larger part is not the one being aimed at. Covered by
  `the_interior_seam_is_two_edges_and_not_a_middle`.
- **With a large grab offset.** Grab a 900 px window two-thirds along its title
  bar and it sits ~600 px to the left of the pointer for the whole drag; the
  pointer can be well inside monitor two's top band while the window is still
  wholly on monitor one. Covered by
  `a_drop_tiles_the_monitor_the_pointer_is_over_even_when_the_window_is_not`.

**Decision 2 — the work area travels with the intent instead of being looked up
twice.**

| Option | *What changes:* |
|---|---|
| **`DropIntent` carries the `WorkArea`** (chosen) | the release places the window in the identical rectangle the outline was drawn in — it is not possible to write a version where they differ |
| Re-resolve at release from the pointer | the same rectangle in practice, but the guarantee is an invariant two call sites must agree on rather than a fact about the type |
| Re-resolve at release from the window | the bug decision 1 rejects, re-entering by the back door |

Carrying the area costs one `WorkArea` (four floats) in a struct that already
exists and lives for the length of a drag. In exchange, "the preview does not
lie" stops being something a test checks and becomes something the code cannot
express otherwise. That is the whole argument, and it is the reason
`maximize_window` and `snap_window_to_zone` were split into public forms (which
resolve the window's monitor — right for a keyboard shortcut or a menu item,
where there is no pointer gesture to speak of) and private `_within` forms
taking an explicit area, which the drop uses.

The third option is not hypothetical: `finish_drag` originally called the public
`maximize_window`, and the reintroduction sweep found that reverting to it was
caught by **no test at all** until the large-grab-offset fixture above was
written. A guarantee no test can fail is not a guarantee.

**Cost accepted.** There is one arguable case on the other side: a user who
drags a window mostly onto monitor two but whose pointer is still, at the moment
of release, back over the seam on monitor one gets monitor one. That is correct
under decision 1 and might momentarily surprise. It is also self-correcting —
the preview outline is showing monitor one at that instant, so the user sees the
answer before committing to it — which is exactly the property decision 2 buys.

**If this is never revisited.** Nothing degrades. The reopening condition is a
tiling gesture that is *not* pointer-driven — a keyboard "snap left" that acts
on a window the pointer is nowhere near, or a touch/pen drag where the contact
point and the intended target differ by design. Those want the public forms
(window's monitor) and already have them; what would need rethinking is only a
gesture that has a pointer *and* wants to ignore it, which no current input path
does.
