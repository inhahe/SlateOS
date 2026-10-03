## TD-C-THE-KEYBOARD-SHORTCUTS-STILL-PRETEND (lane C, 2026-08-21) — RESOLVED 2026-08-21 (see the final note)

**In short:** Alt+F4, Super+D, Super+Up, Super+Left and Alt+Tab appear to work
and do nothing. They change the desktop *the shell has in its head* instead of
asking the compositor to change the real one, and a fraction of a second later
the compositor sends the shell the true picture, which overwrites it. So the
taskbar button flickers and comes back, and the window the user tried to close
is still there.

**Where:** `gui/desktop/src/lib.rs` — `run_desktop_action` (:2171). Six arms
mutate:

| Shortcut | Calls | Should ask for |
|---|---|---|
| `CloseFocused` (Alt+F4) | `remove_window` | `ShellControlAction::Close` |
| `ShowDesktop` (Super+D) | `minimize_window` per window | `Minimize` per window |
| `Maximize` (Super+Up) | `maximize_window` | `Maximize` |
| `RestoreOrMinimize` (Super+Down) | `restore_window`/`minimize_window` | `Restore`/`Minimize` |
| `SnapLeft`/`SnapRight` (Super+Left/Right) | `snap_window` | **no verb exists** |
| Alt+Tab release | `finish_alt_tab` → `focus_window` | `Activate` |

`switch_desktop` (:2064) does the same via `focus_window`.

**Why it only became a bug today.** Until the shell had an event loop none of
this ran against a compositor, so mutating the local model *was* the behaviour
and the tests that assert it were correct. `ShellSession::dispatch` now routes
`Event::Key` into `handle_hotkey` on a live connection
(`TD-C-THE-SHELL-CAN-DRAW-ITSELF-AND-NOBODY-CAN-ASK-IT-TO`, resolved), and
`apply_window_list` — which *replaces* rather than merges, deliberately — lands
on top of whatever the shortcut just did. This is the identical defect the
taskbar had before it was changed to emit `ShellAction::Control`; the pointer
path was converted and the keyboard path was not, because at the time neither
had a caller and only one was being read.

**Reproduce:** run a `ShellSession` against a compositor with one window
focused, press Alt+F4, then let one window-list snapshot arrive. The window is
still listed. There is no test that catches this, because every existing hotkey
test drives `DesktopShell` directly with no compositor to disagree with it —
which is exactly the arrangement that made the bug invisible.

**Proper fix:** `handle_hotkey` returns the requests rather than performing
them, the same way `handle_mouse` returns `ShellAction::Control`. That needs a
return type carrying *many* requests, because `ShowDesktop` minimises every
window on the desktop. Once it does, `remove_window`, `focus_window`,
`minimize_window`, `maximize_window`, `restore_window`, `toggle_maximize`,
`add_window` and `take_window_id` have no caller but the demo and the tests, and
that is the "delete the geometry half of the old private window manager" item
that `TD-C-THE-SHELL-CAN-DRAW-ITSELF-AND-NOBODY-CAN-ASK-IT-TO` leaves open.

**One arm has nowhere to go and needs a protocol verb first.** `ShellControl`
deliberately excludes move/resize, because a shell that can move any window is
the second window manager this whole line of work exists to remove. Snap is not
an exception to that rule but an instance of the rule it already follows: like
`Maximize`, the shell *names an intent* and the compositor computes the
geometry from its own work area. So the fix is `ShellControlAction::SnapLeft` /
`SnapRight` — a named placement, not a rectangle from the client — and not a
`Move { x, y, w, h }`.

**If never fixed:** five of the desktop's most-used keyboard shortcuts are
decorative on a real session, and the shell keeps a second, divergent answer to
"what windows exist" alive purely to receive edits nobody will ever see.

**RESOLVED 2026-08-21.** `handle_hotkey` returns `HotkeyOutcome { consumed,
requests }` and every arm that names a window hands back a `WindowRequest`
instead of editing the local model. `ShellSession` forwards them through the
same `request` helper the pointer path now uses, so a taskbar click and Alt+F4
are literally the same code past the point where the intent is formed. The
protocol verb the snap arm needed was added first, in `207c49e6` —
`ShellControlAction::SnapLeft`/`SnapRight`, named placements rather than
rectangles, dispatching to the compositor's new `snap_window`.

Two things came out differently from the plan above:

- **`ShellAction::Control` was changed to hold the same `WindowRequest`** rather
  than a second struct-variant of the same two fields. The alternative — a new
  type for the keyboard path beside the existing inline variant — would have
  been a second place to forget an action, and the two paths converge on one
  `control_window` call anyway. Six call sites, all in this crate.
- **`switch_desktop` and `finish_alt_tab` now return `Option<WindowRequest>`**
  rather than staying `()` and being special-cased. Both were doing the same
  thing wrong — calling `focus_window`, which is the compositor's decision —
  and both are called from outside `run_desktop_action` (the demo, the tests),
  so hiding the request inside the hotkey path would have left those callers
  still lying.

Guarded by ten reintroduced defects, each of which failed a test that names it:
the close arm mutating locally, Super+D naming only the focused window, Super+D
ignoring the desktop filter, Super+Down never restoring, the shell's SnapLeft
wired to SnapRight, an unfocused-desktop shortcut falling through to the app,
the switcher and the desktop switch focusing instead of asking, the session
dropping a shortcut's requests, and the session sending only the first of a
batch. 2502 tests green, clippy `--all-targets` clean.

Still open, and both now stated in the crate doc: the geometry half of the old
private window manager (`add_window`, `focus_window`, `minimize_window`,
`maximize_window`, `restore_window`, `toggle_maximize`, `remove_window`,
`snap_window`, `snap_window_to_zone`, `take_window_id`) has no caller outside
the demo and the tests and should go. `TD-C-VIRTUAL-DESKTOPS-HIDE-NOTHING`
below was also made visible by this work, and is now resolved.
