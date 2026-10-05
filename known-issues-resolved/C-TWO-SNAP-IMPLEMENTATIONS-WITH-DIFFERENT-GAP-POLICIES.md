## C-TWO-SNAP-IMPLEMENTATIONS-WITH-DIFFERENT-GAP-POLICIES (lane C, 2026-08-18) — FIXED 2026-08-19

**Where:** `gui/desktop/src/snap.rs` and `gui/desktop/src/main.rs::snap_window`
(around line 1700). Fixed as described under **What the proper fix looks like**
below; the gap policy that had to be decided along with it is recorded as
`design-decisions.md` §469.

The desktop has **two** implementations of window snapping, and they disagree.

| | `main.rs::snap_window` | `snap.rs` |
|---|---|---|
| Wired up? | **yes** — this is what runs | **no** — `mod snap;` is its only reference |
| Geometry | integers | `f32` |
| Gap between zones | **none** | `ZONE_GAP = 6.0` |
| Layouts | halves and quarters | seven presets, plus a picker |
| Work area | uses `work_area()` | uses `WorkArea` (as of §467) |

Neither is wrong in isolation. The problem is that they are two answers to one
question, only one of which a user can see, and the invisible one is the more
capable. Whoever wires `snap.rs` up must also decide the gap policy for the
whole shell — a 6 px gap and no gap are both defensible, but a desktop where
some snaps have a gap and others do not is not.

**What the proper fix looks like.** Delete `main.rs::snap_window`'s geometry
and route it through a `SnapManager` owned by the shell, seeded from
`work_area()` and updated whenever the taskbar moves or the screen resizes
(`set_work_area`). The `SnapManager` already exposes everything needed:
`hit_test`, `edge_snap_hit`, `snap_window`, `render_overlay`, and a
`SnapHistory` for restore-on-unsnap, which `main.rs` currently has no
equivalent of at all.

**Why it was not done in §467's commit.** Wiring it is a user-visible behaviour
change to the shell's drag handling (gaps appear, the top edge starts
maximising, a zone overlay and layout picker appear during drags), which is a
larger change than the correctness fix it would be riding on, and wanted to land
as its own commit with its own tests. §467 fixed what `snap.rs` *does* on the
grounds that the cheapest moment for two implementations to agree is before
either is wired.

### What the fix turned out to be

`DesktopShell` now owns a `snap: snap::SnapManager`. `snap_window(id, left)` is
a two-line wrapper over a new `snap_window_to_zone(id, preset, zone)`, which is
the only place window geometry is set from a snap. `main.rs` computes no snap
rectangles at all any more. Three further pieces came with it:

| Piece | Why |
|---|---|
| `unsnap_window` / `is_snapped` | `SnapHistory` was there and unreachable; the shell had no restore-on-unsnap at all. |
| `history.remove` in `move_window`, `resize_window`, `remove_window` | A moved or resized window is no longer in its zone, so unsnapping it would teleport a window the user had just placed. A closed window's entry would otherwise leak one per snapped-then-closed window forever. |
| `sync_snap_area()` at the top of every snap | `work_area()` derives from `screen_width`, `taskbar_height` and `appearance`, all **public mutable fields**. A cached work area kept in sync by notification would be one forgotten call site away from tiling a screen that no longer exists, so it is re-read on use instead. |

### Two defects the wiring exposed, both now fixed

1. **`snap.rs` produced zones outside the work area on small screens.** Every
   arm of the layout builder subtracts `ZONE_GAP` before dividing, so at a
   three-pixel-wide work area `(3 - 6) / 2` is a **negative** half-width and the
   second zone starts at x = 4.5 — outside the rectangle it is tiling. Not
   hypothetical: `DesktopShell::new(1, 1080)` is legal and the window-manager
   tests build one at widths down to zero. The existing in-bounds test could not
   see it because it only ran on desktop-sized areas, where the subtraction is
   free. `build` now tries the gap and keeps it only if every zone stays at
   least `ZONE_GAP` in both dimensions, else rebuilds edge-to-edge — applied to
   the built zones, so a preset added later inherits the rule without knowing it
   exists.
2. **Rounding the zone's origin and extent separately put a window off-screen.**
   On a 1921-pixel screen the right half is `x = 963.5, width = 957.5`, which
   rounds to 964 + 958 = an edge at **1922**. `round_rect` now rounds the
   *edges*, giving `964..1921`. The same argument makes two adjacent zones meet
   by construction rather than by arithmetic luck.

### What the reintroduction checks found

Nine defect variants were restored one at a time and the suite re-run. Eight
were caught immediately. The ninth was a test that could not fail:

| Defect restored | Result |
|---|---|
| A rejected zone id leaves the layout switched | **NOTHING FAILED.** `snapping_to_a_zone_that_does_not_exist_changes_nothing` asked for a bad zone of `TwoEqualHalves` — the preset already in force — so the un-done layout switch was invisible. Rewritten to ask for a zone of `SixGrid`, with the "these differ" precondition asserted. |
| Gap kept when it does not fit | caught |
| Gap always dropped | caught |
| Origin and extent rounded separately | caught |
| Move does not end the snap | caught |
| Resize does not end the snap | caught |
| Closing leaks the history entry | caught |
| History recorded after the snap rather than before | caught |
| Work area not re-read before snapping | caught |

The last two are worth naming because both were written *because* of an earlier
sweep habit, not discovered by one: `SnapManager::snap_window` inserts a
zero-geometry placeholder for a window it has not seen, so recording history
after the snap restores to a 0×0 window at the origin; and nothing in the suite
changed the screen size after construction until
`snapping_follows_the_screen_and_the_taskbar_after_they_change` was added.

**Still not wired:** the drag-time affordances — `hit_test`, `edge_snap_hit`,
`render_overlay` and the layout picker. Snapping is now single-implementation
and keyboard-driven; making a *drag* to a screen edge snap is a separate,
genuinely user-visible change and is tracked as its own work.
