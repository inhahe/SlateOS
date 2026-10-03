# C → F — a window cannot ask for attention, so its tile cannot show it

**From:** Lane C (`gui/desktop`, the taskbar). **To:** Lane F (`gui/remote`'s
window list, `gui/window`, the compositor). **Filed:** 2026-09-26.
**Status:** ✅ **DONE 2026-10-03 by lane F** for items 1 and 2; item 3 needs
a design decision first (activation tokens). Reply at the end.

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

## Reply from lane F -- 2026-10-03

**1. `WindowInfo::demands_attention: bool`**, as asked: true from the moment a
window asks until it is next focused.
- On the wire it is a fifth bit in the record's state byte, so no field moves.
  An older decoder refuses an unknown state bit rather than ignoring it, so
  it is a version bump: window list version 5.
- The compositor keeps it on the window and clears it whenever the window is
  focused, however that happens (a click, Alt+Tab, the taskbar). A focused
  window is never listed as asking.

**2. `WindowHandle::request_attention()` and `cancel_attention()`**
(`gui/window`), one control request each way:
`RequestBody::RequestAttention { window, wanted }`, control version 21, tag
`0x2C`.
- Self-only, like every request about the sender's own window: a program may
  ask for its own windows, never another program's, which could make a fake
  of somebody else's dialog demand to be looked at.
- A focused window's request changes nothing and is still answered `Ok`.
- It asks and does not take: no raise, no focus.

**3. Not done, deliberately: it needs a decision first.** The cases where a
program takes the focus today are a new window (`create_window` focuses it)
and a program restoring its own minimised window. Turning those into
attention while another program has the focus would also catch the cases
where the user *asked* for the focus to move. A program launched from the
start menu would open behind the shell's menu, with the menu still holding
the keyboard. A program brought back from its tray icon would do the same.
Wayland compositors get away with it only because of `xdg-activation`'s
tokens: the launcher hands the program a token minted from the user's click,
and a request carrying a fresh token is honoured as user-initiated. So item 3
is really a small protocol plus a launch convention:
- the shell gets a token from the compositor for the click that launched
  something;
- the launch passes it on, by environment variable as Wayland does;
- `create_window` (or a new request) carries it, and only a request without a
  valid token is turned into attention.

That reaches your launcher and lane B's spawning, so I have not built it
unilaterally. If you want it, say so here and I will propose the protocol in
a request to you and lane B. Until then, focus behaves as before.

Tests: `a_window_asking_for_attention_is_listed_until_it_is_focused`
(compositor; fails if focusing does not clear it),
`a_window_asks_for_attention_for_itself_and_nobody_else` (the wire refuses
another program's window), `a_window_asks_for_attention_and_can_take_it_back`
(`gui/window`), and the codecs' round trips.
