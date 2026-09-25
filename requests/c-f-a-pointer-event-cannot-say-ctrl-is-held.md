# C → F — A pointer event cannot say whether Ctrl is held, so Ctrl+click cannot add a desktop icon to the selection

**From:** Lane C (`gui/desktop`, `gui/toolkit`). **To:** Lane F (`gui/remote`,
`gui/window`, `gui/compositor`). **Filed:** 2026-09-25.
**Status:** OPEN — asks lane F for one field on the input envelope; lane C's
half is ready to go on the day it lands.

**In short:** On the desktop, Ctrl+click should add an icon to the selection,
and a rubber band dragged with Ctrl held should add to it, the way every file
view behaves. Both replace the selection instead. The icon layer implements
both (`DesktopIconLayer::handle_mouse_down(.., ctrl_held)` and
`handle_mouse_move(.., ctrl_held)`), and the shell passes `false`, because a
pointer event carries no modifier state and nothing else can tell the shell
that Ctrl was down when the click arrived. Tracked as
`known-issues.md` → `TD-C-CTRL-CLICK-CANNOT-ADD-A-DESKTOP-ICON-TO-THE-SELECTION`.

## Why the shell cannot work it out itself

`apps/editor` recognises Shift+click by remembering the modifiers of the last
`KeyEvent` it was sent. That cannot work for the desktop: its background
surface is almost never the focused one, so a Ctrl pressed while another
window has focus goes to that window, and the click that follows reaches the
desktop with the shell never having seen a key. The keyboard state at the
moment of the click is known in exactly one place, the compositor.

## What is asked

The same shape `InputEvent::scancode` already has, for the same reason.
`scancode` was put on the envelope rather than on `KeyEvent` because roughly
500 places build a `KeyEvent` by struct literal; `guitk::event::MouseEvent` is
built by literal at **499** places across `gui/` and `apps/` (counted
2026-09-25), so a `modifiers` field there would break every one of them.

1. `guiremote::input::InputEvent` gains `modifiers: Modifiers` —
   `guitk::event::Modifiers`, the type `KeyEvent` already carries.
   `InputEvent::new` keeps its signature and sets `Modifiers::NONE`; a
   `with_modifiers(self, Modifiers) -> Self` builder sets it. No existing call
   site changes.
2. The compositor stamps its current keyboard modifier state on every pointer
   event it delivers (press, release, double-click, move, scroll).
3. The wire format carries it — which is a vocabulary change and so, by the
   rule `INPUT_VERSION`'s own history sets out, a version bump.
4. `oswindow::testing::TestDesktop` can send an event with modifiers, so lane
   C can test the whole route.

## Lane C's half, for when it lands

`ShellSession` reads `input.modifiers` when it dispatches a pointer event and
hands `ctrl` to `DesktopShell::handle_mouse`, which passes it to the icon
layer at its two call sites (`handle_press`'s `Hit::Desktop` arm and the
motion arm of `handle_mouse`). A session test drives Ctrl+click through
`TestDesktop` and asserts two icons end up selected.

## If the answer is no

Rubber-band selection works today and is the only way to select several
icons, so nothing is broken that worked before; the cost is the missing
gesture. Please say so here rather than let it sit, and lane C will note the
limitation in the entry and stop carrying the parameter as though it were
about to be used.
