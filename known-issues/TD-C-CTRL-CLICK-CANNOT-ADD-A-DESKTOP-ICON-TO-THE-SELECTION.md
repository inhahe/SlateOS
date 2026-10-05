## TD-C-CTRL-CLICK-CANNOT-ADD-A-DESKTOP-ICON-TO-THE-SELECTION -- FIXED 2026-10-05

**Status:** FIXED 2026-10-05 (lane C). Lane F's field came first
(2026-10-03, input protocol 9): every pointer event carries the modifiers
held, read as `oswindow::EventLoop::modifiers()` while handling it. Lane C's
half: `ShellSession::pointer` hands them to the new
`DesktopShell::handle_mouse_with`, which passes Ctrl to the icon layer on a
press on the desktop and while a rubber band moves (`handle_mouse` is that
with none held). Test: `ctrl_click_adds_a_desktop_icon_to_the_selection`,
through `TestDesktop` -- a plain click selects one icon, a Ctrl+click adds a
second, a plain click replaces both. Moves to `known-issues-resolved/` once
boot-tested on main.

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
