## 567. The volume keys act, on their own surface, off a clock the shell keeps itself

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** pressing the volume key on a keyboard now actually changes the
volume and puts a small indicator on screen that fades away by itself. Three
smaller choices inside that had a real alternative on the other side, and this
records them: the indicator gets its *own* window rather than sharing the one
the start menu is drawn on; the volume keys are matched with Shift/Ctrl/Alt/the
Windows key *ignored* rather than requiring none of them held; and the shell
counts elapsed time for the fade with a counter of its own rather than reading
the machine's clock.

### 1. A fourth surface, not a fourth thing drawn on the popup surface

The shell already owns three windows — wallpaper, taskbar, and one full-screen
window the menus are drawn on. Adding the overlay to that third one was the
cheap option and is wrong on two independent counts, either of which alone
settles it:

- **They want opposite answers about the mouse.** The menu surface is
  full-screen *precisely* so that a click on bare desktop reaches the shell and
  closes the menu. The overlay must never take a click at all — a volume
  indicator that swallowed the press aimed at the document under it is worse
  than no indicator (§566). One window cannot be both.
- **They come and go on unrelated schedules.** An overlay can appear over an
  open start menu and expire while it is still open. A shared surface would have
  to be mapped whenever *either* wanted it, so a volume fade over an open menu
  would leave the menu's invisible full-screen click-catcher up after the menu
  closed, eating the next click on bare desktop.

*Rejected: one surface, with the overlay drawn into the popup tree.* Cheaper by
one window and one visibility reconciliation, and wrong as above.

### 2. Modifier-agnostic bindings, expanded at the grab

`VolumeUp` has one meaning and no other job — unlike `Tab`, which is Alt-Tab to
the shell and a tab character to everything else. So the binding table matches
it with modifiers ignored, and Shift+VolumeUp turns the volume up like every
other spelling of the press.

But a *grab* (a standing claim on a chord, honoured whoever has the keyboard)
names an exact chord, and the protocol has no "any modifier" spelling. Two ways
to close the gap:

- **Expand in the shell** (chosen): the three keys are enumerated over all
  sixteen modifier combinations, so `global_chords()` returns 48 more entries
  and the compositor is told about each. Costs 48 hash-map entries once at
  startup, and — the part that decided it — keeps the two both-directions tests
  meaningful: *every grabbed chord is one the shell acts on*, and *every chord
  the shell acts on is one it can hear*. Both are sweeps over the grab list, and
  both stay exact.
- *Rejected: a wildcard in the protocol.* A `GrabKey` meaning "this key with
  anything held" would put a special case in the compositor's keystroke path —
  which every keystroke on the desktop goes through — to save the shell 48
  entries it builds once. It would also stop the grab table being comparable to
  the binding table by set membership, which is exactly what those two tests do.

### 3. The shell's own monotonic counter, not a wall clock

`OsdManager` works in absolute millisecond stamps, because a timeout is a
deadline and a deadline cannot be computed from a delta. But the shell holds no
clock — deliberately, so it can be tested without also controlling the machine's
time — and the session receives frame *deltas*, not stamps.

Chosen: `DesktopShell` keeps `osd_clock_ms`, advanced by `advance_osd(dt)` from
the frame handler, and `show_osd` stamps from it. The *origin* of those stamps
is arbitrary, since every one the manager compares is one this counter issued,
so "milliseconds since this shell was constructed" answers every question asked
of it.

Its one apparent hazard is that it stands still while the desktop is idle. That
turns out to be exactly right rather than merely tolerable: nothing is scheduled
while the desktop is idle, so no deadline can fall due in the gap, and
`oswindow` drops a window's delta origin when it is ticked without re-arming —
so the first frame after a park reports the length of *that frame*, not the
length of the pause, and the pause never lands on an overlay's deadline at all.

*Rejected: thread `now_ms` through `handle_hotkey`.* It is the honest signature
— the handler really does need to know the time — but it is the wrong place to
put the requirement: forty call sites would have to supply a clock, thirty-nine
of which have no interest in one, to serve two arms of one match.

*Rejected: `set_current_time` pushed from the session each pump,* as
`NotificationPane` and `SecurityDialog` take it. That is the existing precedent,
and it was not followed because those two want the *wall* clock — they show a
date and an age — whereas this wants only elapsed time; and a push-per-pump
scheme is one forgotten call site away from an overlay that never expires.

### The fix this uncovered

`OsdOverlay::tick` moved `phase_start` to `now_ms` on every phase transition,
which forgave the overshoot. That made the phases fail to partition time — every
overlay outlived its configured life by up to one frame per phase — and defeated
the re-entry loop the function is written around: with `elapsed` recomputed as
zero, a single ten-second tick advanced by exactly one phase, so an overlay
needed three more frames to retire whatever its own clock said. It now advances
`phase_start` by the phase's own duration. Two tests pin it.

**Where it lives.** `gui/desktop/src/lib.rs` — `DesktopShell::osd`,
`osd_clock_ms`, `show_osd`, `advance_osd`, `render_osd`, `DesktopAction::Volume`
/ `ToggleMute`, `MODIFIER_AGNOSTIC_KEYS`, `ALL_MODIFIER_SETS`, `global_chords`.
`gui/desktop/src/session.rs` — the `osd` surface, `osd_shown`, `paint_chrome`,
`anything_moving`, `step_frame`, `resize_display`.
`gui/desktop/src/notif_pane.rs` — `set_volume`, `adjust_volume`, `toggle_mute`,
`is_muted`. `gui/desktop/src/osd.rs` — `OsdOverlay::tick`.
