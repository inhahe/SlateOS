# C → F — a window cannot ask for attention, so its tile cannot show it

**From:** Lane C (`gui/desktop`, the taskbar). **To:** Lane F (`gui/remote`'s
window list, `gui/window`, the compositor). **Filed:** 2026-09-26.
**Status:** OPEN — nothing is broken while it waits; a window that wants the
user simply has no way to say so.

**In short:** the Aero reference's taskbar draws a window that wants the user
-- a chat with a new message, a finished download, a dialog behind other
windows -- as an amber, gently pulsing tile (`aero-task is-alert`), and every
desktop has the same thing (Windows' flashing button, X11's urgency hint,
Wayland's `xdg-activation`). SlateOS's taskbar now draws the reference's other
tile states (design-decisions §1400) but cannot draw this one: nothing in the
window list says a window is asking.

## What is asked

1. **A field on `window_list::WindowInfo`**: `demands_attention: bool`, true
   from the moment a window asks until it is next focused. On the wire, an
   append at the end of the record, so an older reader skips it -- or however
   lane F prefers to version it.
2. **A way for a program to ask**: `gui/window`'s window handle gains
   `request_attention()` (and, if you want it, `cancel_attention()`). A
   focused window asking should change nothing -- it already has the user.
3. **The compositor's own use of it**, if you agree: a window that asks to be
   focused while another program holds focus -- the focus-stealing case -- is
   marked as asking for attention instead of being raised. That is how Wayland
   compositors answer `xdg-activation` without a token, and it turns a
   rudeness into a request.

## What lane C does with it

The tile goes amber and pulses (the reference's `aero-alert-pulse`), and
holds still under reduced motion; the pulse stops when the window is focused.
Nothing else: no sound, no notification -- a tile is a door, not an alarm.

## What happens until it is done

Nothing breaks. Windows that want attention cannot get it, as today.
