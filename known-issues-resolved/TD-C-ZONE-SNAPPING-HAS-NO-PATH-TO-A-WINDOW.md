## TD-C-ZONE-SNAPPING-HAS-NO-PATH-TO-A-WINDOW (lane C, 2026-08-21) — RESOLVED 2026-08-21

**In short:** The desktop shell contains a complete, tested implementation of
multi-window tiling layouts — halves, thirds, quadrants, a six-cell grid —
computing exactly where each window in a layout goes. Nothing calls it, and
nothing can: the only tiling the shell can actually ask the compositor for is
"left half" or "right half". A user cannot reach the other five layouts, because
there is no message that would carry the request.

**Where.** `gui/desktop/src/snap.rs` (2293 lines) holds the layouts
(`SnapLayoutPreset::{TwoEqualHalves, ThreeColumns, TwoThirdsLeft, TwoThirdsRight,
FourQuadrants, ThreeLeftTwoRight, SixGrid}`, `snap.rs:143`), the gap policy (`ZONE_GAP = 6.0`,
dropped when a zone would be too small for it), the snap history (record /
restore / remove), and the overlay and zone-picker render trees. It has its own
test module, which is why the deletion of the shell's private window manager
took the *shell-level* snap tests with it and left this module intact: its
properties are still pinned, they are just pinned in a library with no caller.

The gap is on the wire. `ShellControlAction` (`gui/remote/src/control.rs:257`)
offers `SnapLeft` and `SnapRight` and nothing else, deliberately: its doc
records that the two are separate variants rather than `Snap(Edge)` so that the
wire encoding stays one byte per action with no payload. The compositor
implements those two at `gui/compositor/src/lib.rs:4390` (`snap_window(window_id,
SnapEdge)`), dispatched at 6200–6201, against its own two-variant `SnapEdge`
(`compositor/src/lib.rs:546` — `Left`, `Right`). So even if the shell picked a
zone, it could not name it, and adding a zone verb means deciding what to do
about the one-byte-per-action rule.

**Why it wasn't done in the same change.** The change that exposed this was the
deletion of the shell's private window-manager geometry — the shell keeping its
own window rectangles, which `apply_window_list` overwrote unread on the next
compositor snapshot. Teaching the compositor a zone vocabulary is a protocol
change plus a compositor feature, in two different lanes' trees, and folding it
into a deletion would have made a clean removal unreviewable. Deleting `snap.rs`
instead was rejected: it is correct, tested geometry that the feature will want
verbatim, and re-deriving the gap policy later would be strictly worse than
keeping it.

**The proper fix,** in order:

1. Extend `ShellControlAction` with a zone verb — `SnapToZone { preset, zone }`
   or equivalent — in `gui/remote` (lane C).
2. Teach `gui/compositor`'s `snap_window` to resolve a preset + zone index into
   a rectangle against its own display bounds. The compositor is the only party
   that knows the bounds, so the *rectangle* must be computed there.

   **The layout arithmetic has to move somewhere both crates can see it, and
   where is an open fork.** The compositor needs it to place the window; the
   shell needs it too, because its zone picker draws the zones the user aims
   at — so it cannot simply be handed to the compositor and deleted from the
   shell. `gui/compositor` does not depend on `gui/desktop` and must not start
   to (the dependency runs the other way round conceptually, and nothing in the
   tree depends on `desktop` today, which is a property worth keeping). The two
   candidates:
   - **Into `gui/remote`**, beside `Layer`, `WindowInfo` and `CursorShape` —
     the crate that already holds the vocabulary both ends share. Fits the
     existing pattern, no new crate. Against: `gui/remote` is a *protocol*
     crate, and `SnapZone` carries a `String` label and returns a `Vec`, which
     is heavier than anything else in there.
   - **A new `gui/zones` crate.** Honest about what it is, keeps the protocol
     crate to protocol. Against: ~300 lines of arithmetic is thin for a crate,
     and it adds a node to the graph for one caller on each side.

   Resolve this *before* writing the compositor half — picking wrong means
   moving the code twice.
3. Design the wire verb against `ShellControlAction`'s one-byte-per-action
   rule (`gui/remote/src/control.rs:307`, `as_byte`/`from_byte`). The 22
   distinct (preset, zone) pairs across the seven presets all fit in the unused
   byte space above 6, so the rule can be kept exactly rather than bent: the
   byte still fully determines the action, and no reader or writer of the frame
   grows a special case. A `SnapToZone { preset, zone }` with a separately
   encoded payload would be the thing the enum's doc explicitly warns against.
4. Wire the shell's zone picker (`snap.rs`'s overlay/picker render trees) to
   emit that request from `handle_mouse`/`handle_hotkey`, and drop the
   snap-history bookkeeping from the shell — restoring a snapped window is
   already the compositor's (`restoring_a_snapped_window_returns_it_to_where_it_was`).

**If never fixed:** no user-visible breakage — the two-half snap works and is
the one most people use — but 2293 lines of tested code stay unreachable, which
invites the next reader to assume the feature exists because the tests are
green. `design.txt` does not mandate zone presets, so this is a feature the
project chose to build and has not yet connected, not a spec violation.

**RESOLVED 2026-08-21.** All four steps done, in three commits:

| Step | Commit | What landed |
|---|---|---|
| 1, 3 | `d17ef6149` | `SnapSlot` (`gui/remote/src/zones.rs`) and the `SnapToZone` wire verb. The one-byte rule is **kept, not bent**: the 22 (preset, zone) pairs occupy bytes above the existing actions, so the byte still fully determines the action. |
| 2 | `d04192fc5` | The compositor's `SnapTarget` / `snap_window_to_zone` / `place_snapped` / `zone_rect`, resolving a slot against `display_manager.virtual_bounds()` — the bounds only it has. |
| 4 | `7d138bb51` | Super+Z opens the chooser; a press on a zone emits `ShellControlAction::SnapToZone(slot)` for the window focused now. The snap history is gone from the shell, as the step said. |

**The open fork in step 2 was resolved into `gui/remote`**, not a new
`gui/zones` crate — see `design-decisions.md` §507 for the argument (a zone
table *is* protocol: `SnapSlot`'s index ranges and the geometry those indices
name are the same fact stated twice, and the one place they cannot drift is the
same file). The `String` label objection was answered by the type carrying a
`&'static str`.

Eight new tests in `gui/desktop/src/pointer_tests.rs`, each proved to bite by
reintroducing the defect it names (twelve defects, twelve deterministic
failures naming the test back): hit-test order, work-area staleness, the
empty-desktop guard, the zone id, the focused window, the dismissal, the
visibility gate, the hover, the thumbnail grid, and all three close paths.

**What did not land with it:** the *other* way desktops offer this — drag a
window to an edge and drop — still has no drag to fire on. See
`TD-C-EDGE-DRAG-TILING-HAS-NO-DRAG-TO-FIRE-ON` below.
