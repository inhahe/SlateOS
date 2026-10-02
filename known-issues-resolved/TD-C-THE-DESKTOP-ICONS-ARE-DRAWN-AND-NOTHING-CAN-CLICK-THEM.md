## TD-C-THE-DESKTOP-ICONS-ARE-DRAWN-AND-NOTHING-CAN-CLICK-THEM -- FIXED 2026-09-14

**Date:** 2026-09-14. **Lane:** C. **Created and closed by this lane the same day.**

**Status: FIXED.** The pointer reaches the layer: a press selects, a drag moves
and the release writes the new position, a double-click asks the shell to launch
what the icon points at. Four tests in `pointer_tests.rs`, all through
`handle_mouse` rather than against the layer, because the layer's own
interactions were never in doubt -- the route to them was what did not exist.
Proved by reintroduction: unrouting the press fails three of the four.

One thing the fix had to *not* break, which three existing tests caught: a press
on bare desktop with nothing selected still answers `Pass`. The first version
consumed every desktop press on the theory that a rubber-band is a gesture in
progress. It is, but the gesture does not need the press claimed -- the release
reaches the surface either way -- and `the_bare_desktop_is_not_the_shells_to_consume`
was right that claiming it is a lie about what changed. The rule is now "consume
what was acted on": the selection before and after decides.

Still open, and deliberately: `ctrl_held` is passed as `false` everywhere,
because `MouseEvent` carries a position and a kind and nothing else, so
ctrl-click to extend a selection has nowhere to get its answer. The keyboard
state would have to be tracked alongside, as `apps/editor` does.

**In short:** the desktop now draws its icons -- This PC, Recycle Bin,
Documents, Home -- and they are a picture. Clicking one does nothing, dragging
one does nothing, double-clicking one opens nothing. They were not drawn at all
before today, so this is a dead control **I introduced**, on the day I spent
removing dead controls from the file manager, the editor and the taskbar.

**Why it is filed rather than fixed on the spot.** The remaining work is a real
change to `DesktopShell::handle_mouse`, not a line:

* `Hit::Desktop => ShellAction::Pass` is the seam -- a press on bare desktop is
  exactly where the icons now live -- but the layer needs **press, move, up and
  double-click**, and a press that starts a `PendingDrag` with no matching
  `handle_mouse_up` leaves the layer in a non-idle state permanently. Wiring
  press alone would be worse than wiring none.
* The shell collapses `Press` and `DoubleClick` into one match arm
  (`gui/desktop/src/lib.rs`, the `MouseEventKind::Press(button) |
  MouseEventKind::DoubleClick(button)` arm), so "activate" cannot be told from
  "select" without separating them.
* `DesktopIconLayer::handle_mouse_down` takes `ctrl_held`, and the shell's
  pointer path tracks no modifier state at all. Ctrl-click to add to a
  selection has nowhere to get its answer from today.

**What works already**, so the gap is precisely this and no wider: positions
persist and are restored (`read_positions`/`write_positions`, six tests, clamp
proved by reintroduction), the layer is drawn between the wallpaper and the
widgets, and every interaction is implemented and tested *inside* `icons.rs`.
It is only the route from the compositor's pointer to that layer that is
missing.

**`IconEvent::Activate(_, IconAction::OpenPath(p))` should become
`ShellAction::Launch(p)`** -- the shell already has that action and the session
already queues launches from it, so the activation path is one mapping once the
events arrive.

**~~Do not delete `/proc/deskicons` yet.~~ DONE 2026-09-14 -- the ordering was
met and the kernel side is deleted.** A-Q8 ordered this deliberately: lane C
wires first, lane A deletes after, because positions reaching disk is not the
same as a desktop the user can use. Lane C's shell now reads positions at
session start, writes on drag release, and routes press/drag/double-click
through `handle_mouse`, with the clamp and the routing each proved by
reintroduction. Lane A then removed `fs::deskicons`, `/proc/deskicons`, the
kshell command and its self-tests -- 893 lines across ten files.

The instruction is struck through rather than deleted because it was correct
when written and the record of *why* the ordering existed is worth keeping. An
unstruck 'do not delete yet' beside a thing already deleted is how a reader
concludes somebody jumped the gun.
