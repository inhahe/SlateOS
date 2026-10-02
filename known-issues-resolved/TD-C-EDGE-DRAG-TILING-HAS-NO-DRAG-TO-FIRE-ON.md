## TD-C-EDGE-DRAG-TILING-HAS-NO-DRAG-TO-FIRE-ON (lane C, 2026-08-21) — RESOLVED 2026-08-21

**In short:** There are two ways every desktop lets you tile a window: press a
keyboard shortcut and pick a slot from a menu, or drag the window to the edge
of the screen and let go. The first now works (Super+Z). The second does not,
and not because the tiling is missing — the code that says "a drop against the
left edge means the left half" is written and tested. What is missing is anyone
to tell the shell that a drag is happening. The shell never sees a window being
moved; the compositor does that by itself and does not mention it.

**Where.** `gui/desktop/src/snap.rs`: `SnapManager::action_for_edge(x, y)`
turns a cursor position near an edge or corner into the
`ShellControlAction` that drop should send, and `edge_snap_hit` is the other
half — the overlay preview drawn while the cursor is there. Both are public,
both are covered by the module's own tests
(`an_edge_drop_asks_for_the_tile_that_edge_means`,
`an_edge_drop_ignores_the_layout_the_picker_has_selected`), and **neither has a
caller anywhere in the tree.**

The reason is structural, not an oversight: the shell has no window dragging at
all. `grep` for a drag-start/drag-move/drag-end in `gui/desktop` finds nothing,
because interactive moves are the compositor's — it owns the pointer grab and
the window geometry, and `gui/remote`'s event stream carries no "the user is
moving window N, the pointer is at (x, y)" message for the shell to hook.

**Why it was kept rather than deleted.** It is the correct half of a gesture
whose other half is a protocol addition in the same lane's tree. Deleting it
would mean re-deriving the edge/corner thresholds and the deliberate rule that
an edge drop ignores the layout the picker has selected (dragging left means
*the left half*, whatever grid is chosen — see `design-decisions.md`), which is
exactly the "re-writing it later would be strictly worse" argument that §506
already made about `snap.rs` as a whole.

**The fix as originally proposed — and why it was rejected.** The plan above
was to teach `gui/remote`'s event stream a "the user is moving window N, the
pointer is at (x, y)" notification, so the shell could keep its two functions
and drive them from the compositor's grab. That was written before anyone
counted what it costs: it puts a **socket round trip inside the part of the
gesture the user is watching.** The preview has to follow the cursor at pointer
rate, and every frame of it would be compositor → shell → compositor. The
compositor already holds the grab, the geometry and the display bounds; it can
answer the same question locally in a function call. The shell's copy was
therefore **deleted, not connected** — see `design-decisions.md` §508.

**RESOLVED 2026-08-21.** Four commits, each proved by reintroducing every
defect its tests name and confirming a deterministic failure that names the
test back (39 defects, 39 failures):

| Stage | Commit | What landed |
|---|---|---|
| 1 | `63bf975ba` | The rules moved into the protocol crate: `ScreenEdge` (8 variants; the compositor's own 2-variant `SnapEdge` already had the name), `edge_at`, `drop_at` and `EdgeDrop` in `gui/remote/src/zones.rs`, beside the `SnapSlot` table they resolve against. 10 tests, 12 defects, 214 guiremote tests green. |
| 2 | `5baa11864` | The compositor acts on a drop: `drop_intent` off the live `DragState`, routed through the existing `maximize_window` / `snap_window_to_zone` so the fixed-size refusal, the restore rectangle and the `WindowResized` notification all still apply. 10 tests, 7 defects, 290 green. |
| 3 | `3aa6e8b43` | The preview: a translucent wash plus a border, damaged up and down as the intent changes, torn down on release and on destroy. Included a real bug fix — `rect_outline`'s four bands **overlapped** on rectangles shorter than twice the border, double-blending exactly the small previews where it shows most. 9 tests, 10 defects, 299 green. |
| 4 | `9771783c4` | The shell's copy deleted — 498 lines out of `gui/desktop/src/snap.rs` (`SnapEdge`, `detect_edge`, `EdgeSnap`, `edge_to_default_snap`, `edge_snap_hit`, `action_for_edge` and their tests), module docs in `snap.rs` and `lib.rs` repointed at `guiremote::zones::drop_at`. |

**The deleted code had already drifted**, which is the argument against keeping
a second opinion around: the shell's `edge_to_default_snap` mapped a drop
against the **top** edge to the *left half*, where the surviving rules maximize.
Nothing caught it, because the shell's tests only asserted that the result named
a zone that exists. The replacements in `guiremote` compare the returned
rectangle against the layout's own zone, so the same drift cannot recur silently.

**Point 3 of the old plan — should the full zone overlay appear during a drag,
as FancyZones does? — was not carried forward as an open question.** What
shipped shows the single rectangle the drop will produce, which is what the
Windows and GNOME edge gestures show; the grid overlay is a *different* feature
(drag-into-a-grid, not drag-to-an-edge) and would be a new roadmap item, not an
unanswered fork in this one.
