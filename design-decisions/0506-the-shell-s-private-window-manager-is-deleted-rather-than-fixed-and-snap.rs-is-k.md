## 506. The shell's private window manager is deleted rather than fixed, and `snap.rs` is kept as a library with no caller

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop shell used to keep its own copy of where every window
was, and its own methods for moving them — open a window, raise it, maximize it,
snap it to half the screen. None of it ever had any effect on a live desktop:
the compositor sends the shell a fresh list of every window several times a
second, and receiving that list *replaces* everything the shell believed. So
every edit the shell made to its own copy was thrown away, unread, moments
later. The code has been deleted instead of repaired. One piece was kept even
though nothing calls it: the file that computes multi-window tiling layouts,
because it is correct and the feature will want it.

### What was deleted

From `gui/desktop/src/lib.rs`: `add_window`, `take_window_id`, `take_z`,
`compact_z_order`, `remove_window`, `focus_window`, `minimize_window`,
`maximize_window`, `restore_window`, `toggle_maximize`, `move_window`,
`resize_window`, `snap_window`, `snap_window_to_zone`, `unsnap_window`,
`is_snapped`; the `WindowGeometry` struct and `ManagedWindow::frame_rect`; the
`x`/`y`/`width`/`height`/`restore_geometry` fields of `ManagedWindow`; the
`next_z`, `next_window_id` and `snap` fields of `DesktopShell`, with
`snap_area`, `sync_snap_area` and the three rounding helpers that existed only
to serve them; and `Hit::WindowContent(WindowId)`.

`Hit::WindowContent` is the clearest illustration of the whole problem. It named
the case "the pointer is over a window rather than over the shell's own chrome",
and a live session could **never produce it**: `WindowInfo`, the compositor's
description of a window, carries no geometry at all, so every window's rectangle
in the shell was zero-by-zero, and a zero rectangle contains no point. The
variant was reachable only from tests that had called `add_window` — the door no
live session uses — to give a window a rectangle by hand.

### Why deletion rather than repair

The alternative was to make the shell's copy authoritative: have
`apply_window_list` *merge* into the existing list instead of replacing it, so
that a shell-side edit survived the next snapshot. That is the wrong direction
on two counts.

- **It re-creates the second answer.** Two parties would then hold an opinion
  about where a window is, and they would disagree the moment either moved one.
  The compositor's opinion is the one that reaches the screen, so the shell's
  can only ever be stale — see §504, which settled the same question for
  snapping specifically.
- **The compositor already implements all of it,** with its own tests:
  `snap_window` (`gui/compositor/src/lib.rs:4390`) and the properties
  `the_two_snapped_halves_tile_the_display_with_no_seam`,
  `restoring_a_snapped_window_returns_it_to_where_it_was`,
  `a_window_is_never_snapped_and_maximized_at_once`,
  `a_non_resizable_window_refuses_to_be_snapped`,
  `a_snapped_client_is_told_its_new_size`. Repairing the shell's copy would have
  produced a second implementation of a solved problem, in the process that
  cannot see the display bounds.

What replaced it is nothing at all: a click or a keystroke that wants a window
changed already returns a `WindowRequest` (§505), and the shell finds out what
happened from the next list. `z_order` is no longer a counter the shell bumps —
it is the window's index in the list the compositor sent, which that list emits
bottom-to-top, so the shell's stacking order *is* the compositor's and cannot
drift from it.

Virtual desktops stayed, because they are genuinely shell-local: the compositor
has no notion of them, so there is no second answer to conflict with. The one
thing this required was that a new window list must not reset a window's
desktop — pinned by
`a_window_list_does_not_move_a_window_back_off_its_desktop`.

### Why `snap.rs` was kept with no caller

`gui/desktop/src/snap.rs` is 2293 lines that compute seven multi-window tiling
layouts (`SnapLayoutPreset`), the gap between tiles and the policy for dropping
it when a tile would be too small, the snap history, and the overlay and picker
render trees. Deleting the shell's snap *methods* left it with no caller at all.

- **For deleting it:** dead code is a liability; a reader seeing green tests
  concludes the feature works, and it is not reachable by any user.
- **For keeping it:** it is correct, and its own test module pins every property
  the deleted shell-level tests were pinning, so nothing was lost by deleting
  those. The feature is wanted — it is the shell's answer to Windows'
  FancyZones — and the gap-drop policy in particular is the kind of small
  judgement that is annoying to re-derive. Re-writing it later from scratch
  would be strictly worse than keeping it.

Kept, on the grounds that the cost of the liability is a stale-code smell and
the cost of deletion is losing work that will be wanted verbatim. The smell is
mitigated by naming it in three places rather than leaving it to be discovered:
the crate doc's "What this crate does not do yet" section, the module's own
docs, and `known-issues.md` `TD-C-ZONE-SNAPPING-HAS-NO-PATH-TO-A-WINDOW`, which
records the actual missing piece — the shell-to-compositor protocol has no verb
for "zone 3 of a three-column layout", and `ShellControlAction`'s one-byte-per-
action encoding is a deliberate constraint that adding one has to reckon with.

**Revisit if:** zone tiling is still unreachable when someone next touches
`gui/remote`'s control encoding. At that point either wire it up or delete it —
the one outcome to avoid is a third year of tested, unreachable geometry.
