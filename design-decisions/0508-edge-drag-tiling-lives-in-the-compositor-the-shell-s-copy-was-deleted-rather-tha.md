## 508. Edge-drag tiling lives in the compositor; the shell's copy was deleted rather than connected

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous) — lane C

**In short:** Dragging a window to the edge of the screen and letting go should
tile it — half the screen on the left or right edge, full screen at the top, a
quarter in each corner. Two programs could have implemented that gesture: the
shell (the taskbar-and-desktop program) or the compositor (the program that owns
the screen and the mouse). The shell already had the arithmetic written, but no
way to know a drag was even happening — the compositor moves windows by itself
and tells nobody. The choice was between teaching the compositor to narrate
every mouse move to the shell, or moving the ~200 lines of arithmetic into the
compositor and deleting the shell's copy. The second was taken. The shell's
version was then deleted outright rather than left in place as a spare, because
it had already quietly gone wrong in a way nothing noticed.

### Why the compositor and not the shell

The gesture has three moments — the drag starts, the cursor moves, the button is
released — and the middle one is the problem. A preview rectangle has to follow
the cursor at pointer rate, and the user is looking directly at it. Under the
shell design each of those frames is a round trip: compositor sees the motion →
sends it over the socket → shell computes an intent → sends back an overlay
request → compositor draws. Under the compositor design it is a function call
against state the compositor already holds.

| Option | *What changes* |
|---|---|
| Shell owns it (the original plan in `known-issues.md`) | the preview lags the cursor by one socket round trip, on the one gesture where lag is most visible; `gui/remote` grows a per-motion notification whose only consumer is this feature |
| **Compositor owns it (taken)** | the preview tracks the cursor exactly as the dragged window does, because it is computed in the same event handler; no protocol change at all |

Three facts settled it beyond the latency argument:

1. **The compositor already holds everything the decision needs** — the drag
   grab (`DragState`), the window geometry, and `display_manager.virtual_bounds()`,
   which is the *only* place the screen arrangement is known. The shell would
   have had to be told all three.
2. **The rules were already shared.** Stage 1 put `edge_at` / `drop_at` /
   `EdgeDrop` in `gui/remote/src/zones.rs` beside the `SnapSlot` table they
   resolve against (§507's home), so "the compositor owns the gesture" does not
   mean "the compositor owns a private copy of the rules." Either program can
   still read them; only one acts on them.
3. **The drop routes through the tiling that already existed** —
   `maximize_window` and `snap_window_to_zone` — so the fixed-size-window
   refusal, the restore-rectangle bookkeeping and the `WindowResized`
   notification all apply to an edge drop without being restated. A shell-side
   implementation would have sent a `ShellControlAction` that lands in the same
   two functions anyway, one round trip later.

The counter-argument, which is real: **policy in the compositor is policy the
user cannot replace.** Swapping the shell for a different one leaves edge-drag
behaving exactly as before. That is accepted here because the *layouts* remain
the shell's to choose (Super+Z, §507) and only the edge gesture is fixed — and
because a compositor that cannot answer "where does this window go" without
asking another process is a compositor that cannot function when that process
is not running.

### Why the shell's copy was deleted rather than kept

`gui/desktop/src/snap.rs` had `SnapEdge`, `detect_edge`, `edge_to_default_snap`,
`edge_snap_hit` and `action_for_edge` — tested, public, and called by nothing.
`known-issues.md` had argued for keeping them, on the grounds that re-deriving
the thresholds later would be strictly worse.

That argument did not survive contact with the code. **The shell's copy had
already drifted:** `edge_to_default_snap` mapped a drop against the *top* edge
to the **left half**, where the rules that shipped maximize. Its tests were
green throughout, because they only asserted that the result named a zone that
exists. So the "spare copy" was not a safety net — it was a second, wrong answer
that the test suite endorsed, waiting for someone to wire it up.

| Option | *What changes* |
|---|---|
| Keep it as reference | 498 lines of tested, uncallable, already-wrong code stay in the tree, and the next reader has two answers to choose between |
| **Delete it (taken)** | one implementation, in one place, that the compositor actually runs |

The replacement tests are strictly stronger for the same reason the old ones
were weak: they compare the returned rectangle against the layout's own zone
rather than checking that a name resolves, so the top-edge drift could not
recur silently.

### If this is never revisited

Nothing degrades. The condition that would reopen it is a second shell, or a
user-configurable edge-gesture policy — at which point the *action* (not the
geometry) would need to become a value the shell can set, and the natural shape
is a per-edge table the compositor reads at startup rather than a per-motion
notification. That is a much smaller protocol addition than the one rejected
here, and it keeps the preview local either way.
