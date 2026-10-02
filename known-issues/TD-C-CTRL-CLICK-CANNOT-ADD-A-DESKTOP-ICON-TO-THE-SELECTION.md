## TD-C-CTRL-CLICK-CANNOT-ADD-A-DESKTOP-ICON-TO-THE-SELECTION

**Status:** OPEN — 2026-09-25. Waits on lane F for one field:
`requests/c-f-a-pointer-event-cannot-say-ctrl-is-held.md`.

**In short:** On the desktop, Ctrl+click replaces the icon selection instead
of adding to it, and so does a rubber band dragged with Ctrl held. The only
way to select several icons is to drag a rubber band around them. Nothing that
worked has stopped working; a standard gesture is missing.

**Where:** `DesktopIconLayer::handle_mouse_down` and `handle_mouse_move`
(`gui/desktop/src/icons.rs`) take `ctrl_held` and implement both gestures;
`DesktopShell` passes `false` at both call sites in `gui/desktop/src/lib.rs`
(`handle_press`'s `Hit::Desktop` arm, and the motion arm of `handle_mouse`).

**Why not fixed here:** a pointer event carries no modifier state —
`guitk::event::MouseEvent` is `{x, y, kind}` — and the shell cannot recover it
from key events the way `apps/editor` does for Shift+click, because the
desktop's surface is rarely focused: a Ctrl pressed while another window has
focus goes to that window, and the click that follows reaches the desktop
with the shell never having seen a key. Only the compositor knows the
keyboard state at the moment of the click.

**The fix:** the compositor stamps its modifier state on every pointer event,
on the input envelope (`guiremote::input::InputEvent`) rather than on
`MouseEvent`, for the reason `InputEvent::scancode` is on the envelope:
`MouseEvent` is built by struct literal at 499 places across `gui/` and
`apps/`. Then `ShellSession` hands `ctrl` to the shell and the shell to the
layer, with a session test that drives Ctrl+click through `TestDesktop`.
