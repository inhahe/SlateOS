## 504. Snapping a window is a named edge sent to the compositor, not a rectangle computed by the shell

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** pressing Super+Left should tile the focused window to the left
half of the screen. The desktop shell used to work out the rectangle for that
itself and write it into its own copy of the window list — which the compositor
never saw, so nothing moved. The fix needed the shell to ask the compositor
instead, and the question was *what to ask for*: "put this window at x=0, y=0,
960 by 1080", or just "put this window on the left". We chose the second.

The shell's request vocabulary (`ShellControlAction` in
`gui/remote/src/control.rs`) deliberately has no move or resize. That exclusion
is the line between a shell and a second window manager: a client that can put
any window at any rectangle owns the layout, and then two things own it and they
drift. Adding `SnapLeft`/`SnapRight` looked at first like an exception to that
rule and is in fact an instance of it — `Maximize` was always of this shape.
Every action in the enum names an *intent* and lets the compositor derive the
geometry from bounds only it knows.

- *For a rectangle from the shell:* the shell already computes snap zones for
  its drag-to-edge overlay, so the arithmetic exists; and a rectangle can
  express layouts a fixed vocabulary cannot (thirds, quadrants, a custom grid).
- *Against it, decisively:* the shell's idea of the work area is a copy. It is
  right until a monitor is unplugged, a resolution changes, or the taskbar
  moves — and then the shell tiles a window to half of a screen that no longer
  exists, silently, with no error path to notice on. The compositor's bounds are
  the only ones that cannot be stale. The test the shell now carries for this
  asserts the *edge* that was asked for and nothing about pixels; the tiling
  arithmetic is asserted once, in the compositor, against a display with an odd
  width.

**The state is stored as `snapped: Option<SnapEdge>`, not as a bool beside
`maximized`, and not as the rectangle.** Two bools can represent a window that
is both maximized and snapped, which `restore_window` would then have to pick
between; the `Option` makes that unrepresentable and turns "snapping clears
maximized" into an assignment rather than an invariant to remember. Storing the
edge rather than the resulting rectangle is the same staleness argument one
level down: a stored rectangle is wrong after a display change and cannot be
recomputed, an edge always can be.

**The wire encoding is one byte per action, so left and right are separate
variants rather than `Snap(SnapEdge)`.** A nested payload would make this the
only action carrying one, and every reader and writer of the frame would grow a
special case for a two-valued field. The cost is two enum variants instead of
one; the guard is `ShellControlAction::ALL` plus a test that decodes every byte
value and checks the count, so adding a variant without wiring it breaks the
build.
