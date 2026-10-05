## 507. The zone *shapes* move into the protocol crate; the shell keeps only the chooser, and pulls its work area on use

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous) — lane C

**In short:** The desktop has a "snap layouts" feature — press Super+Z and a
grid of rectangles appears over the screen; click one and the window you were
using jumps to fill it. Until now the grid was a picture and nothing more: the
shell drew it, worked out which rectangle you clicked, and then had nowhere to
send that. It cannot move windows — only the compositor (the program that
actually owns the screen) can. The question was what the shell should send.
The answer taken is that it sends a *name* ("zone 3 of the three-column
layout"), never a rectangle, and that the table saying what zone 3 *is* lives
in the crate both programs already share, so neither can hold a different idea
of it. Two smaller calls come with it: the layout menu is not shown until you
reach for it, and the shell re-measures the screen at every gesture rather than
being told when it changes.

### The decision that `snap.rs`'s module doc points at: where the shapes live

`SnapLayoutPreset` and the `SnapZone`s it builds over a `WorkArea` moved from
`gui/desktop/src/snap.rs` into `gui/remote/src/zones.rs` — the *protocol* crate
(`guiremote`), the one that already defines every message the shell and the
compositor exchange — and are re-exported from `snap.rs` so shell code reads
unchanged.

They had to move somewhere: a named zone means nothing unless both ends compute
the same rectangle for it, and the two ends are two crates. The three homes
considered:

| Home | *What changes* |
|---|---|
| Stay in `gui/desktop` | the compositor would depend on the desktop shell to resolve a message the shell sent it — a dependency backwards through the protocol |
| A new crate, `gui/zones` | one more crate in the workspace, whose only two users are the two that already share `guiremote` |
| **`guiremote` (taken)** | the layout tables sit beside the wire verb that names them, and version together by construction |

The deciding argument is that a zone table *is* protocol. `SnapSlot` encodes
`(preset, index)` in one byte because `ShellControlAction`'s encoding is one
byte per action; the index ranges that make that encoding work (`first_slot`,
`from_index`) and the geometry the index refers to are the same fact stated
twice, and the one place they cannot drift apart is the same file. A separate
crate would have been defensible, but it buys isolation that nothing wants —
neither user exists without `guiremote` — at the price of a third place to look.

**Against:** `guiremote` is meant to be small, and it now carries ~500 lines of
tiling geometry that is not, strictly, a message format. Accepted: the
alternative is a crate that would exist only to avoid that appearance.

### The layout picker is summoned, not shown with the overlay

The chooser could have opened with the layout menu already up — one gesture
instead of two. It does not, and the reason is arithmetic rather than taste.
The picker is a 340×284 panel centred on the top of the work area; on a
1000×800 screen the work area is 1000×760 and the panel occupies x 330..670,
y 20..304. SixGrid's top-middle zone has its centre at (500, 190) — *inside*
the panel. Always-up, a click aimed at the middle of a drawn zone would select
a layout instead of placing the window, and every layout with an odd number of
top-row zones has the same collision.

*What changes:* the panel appears when the cursor reaches the top edge band
(the existing `is_in_picker_trigger`, 16 px), and goes away when it leaves both
band and panel. Choosing a layout does not close the chooser, because choosing
a layout is not choosing a zone.

### The work area is pulled on use, not pushed on change

`SnapManager` holds the rectangle it tiles. Keeping it current could be done by
notifying it whenever the screen or the taskbar changes, or by re-reading it at
the top of every gesture. The second was taken.

`screen_width`, `taskbar_height` and `appearance` are all *public fields* that
anything may assign, and `work_area()` derives from all three. A push scheme is
therefore only correct while every writer remembers to call the notifier — and
the failure is silent: zones drawn against a screen size that no longer exists,
with the window placed exactly where the stale rectangle says. `sync_snap_area()`
re-seeds instead, at the top of `handle_press`, of the motion path, and of
`toggle_zone_overlay`, guarded on inequality because `set_work_area` rebuilds
the layout and would otherwise clear the hover on every mouse move.

*Cost:* three float comparisons per gesture, against a correctness property
that cannot be broken by adding a call site. `the_chooser_tiles_the_work_area_
and_not_the_screen` pins it by moving the taskbar and asserting the zones
follow at the next gesture.

### If this is never revisited

Nothing degrades. The one thing to watch is the reverse of the first decision:
if a third program ever needs zone geometry *without* speaking the shell
protocol, `guiremote` becomes the wrong home and `gui/zones` becomes right.
Nothing on the roadmap implies such a program.
