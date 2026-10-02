## 563. A panel opened from a button stops short of that button, so the second press closes it

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The desktop grew a bell in the system tray — the little clock
corner at the right end of the taskbar — that opens the notification panel.
The panel slides in over the right-hand third of the screen. Written the
obvious way it covered the whole height of the display, bell included, so the
user's second press on the bell landed on the panel's own body and did
nothing: the only ways back out were the Escape key, a chord almost nobody
knows, or a click on the dimmed part of the screen. The decision is to make
the panel stop at the top edge of the taskbar, leaving the bar — and the bell
— visible and live while the panel is open, so that pressing the bell again
closes it.

**The rule, stated generally.** *A surface opened by a control must not cover
that control.* Every popup this shell already had obeys it by accident: the
start menu grows upward from the start button, the calendar hangs above the
clock. The notification pane was the first one written as a full-height
column, and full height on a desktop whose taskbar is at the bottom means
"over the button that opened me".

The failure it produces is quiet, which is what makes it worth a section. The
pane still *works*; every one of its own controls responds. What is broken is
only the way out, and only by the route the user just used to get in — so the
symptom is "I pressed the bell twice and it stuck", and there is nothing on
screen that says which press was ignored or why.

**Decision.**

- `DesktopShell::notification_pane_height()` returns `taskbar_rect().y`, and
  it is the single source for the number: both `render_notifications` and the
  mouse route in `handle_mouse` read it. Two numbers here would be a pane
  hit-tested somewhere other than where it is painted — the pane derives its
  own column and its scroll bound from whatever height it was last handed.
- While the pane is open, `handle_mouse` forwards to it everything *except* a
  press on the taskbar rect. The bar is neither covered nor dimmed, so an
  event there is a real event on the bar and falls through to `hit_test`.
- `handle_press` gains a dismissal arm beside the ones the start menu, the
  power menu and the calendar already have: with the pane open, any hit that
  is not `Hit::NotificationBell` closes the pane and is spent doing so. The
  bell is excepted because it falls through to its own arm one match later,
  which toggles — without the exception the press would close the pane here
  and reopen it there.

**Alternative considered: keep the pane full-height and special-case the bell
in the mouse route.** The pane would stay a modal sheet over the whole
display, and `handle_mouse` would test `bell_rect()` before forwarding. It is
fewer moving parts. It was rejected because it makes the bell a button that is
*drawn underneath an opaque panel* and still answers presses — the user cannot
see the thing they are clicking, so the affordance is a secret, which is the
defect being fixed rather than a fix for it.

**Alternative considered: leave the bell out and keep Super+N as the only
route.** This is what shipped before today, and the pane's own module doc had
claimed a tray click since the day it was written. A panel whose only entrance
is an undocumented chord is a panel nobody opens; the wallpaper failure that
§562's wiring finally gave somewhere to appear would appear somewhere nobody
looks.

**A consequence worth naming: the taskbar is live while the pane is open.**
Pressing the start button with the pane open closes the pane and does *not*
open the menu — the same "dismissing is what the user aimed at" rule the other
popups follow. That is a deliberate choice over letting the press do both:
one click doing two things the user did not ask for in sequence is how a
desktop surprises somebody.

**The badge, and why it is right-aligned inside a fixed slot.** The bell's
slot is a fixed 24 px square, not a measured width like the clock's, and the
unread count is drawn right-aligned *inside* it — overlapping the glyph when
it is two characters wide rather than widening the slot. The tray is laid out
right-to-left from the display edge, so anything that changes width there
shuffles every window button sideways; a count that did that would move the
whole taskbar each time a notification arrived. The count also stops at `9+`
for the same reason: three characters do not fit, and a slot that grew to fit
them is the thing being avoided.

**Where it lives.** `gui/desktop/src/lib.rs` — `TRAY_BELL_WIDTH`,
`NOTIF_BELL_GLYPH`, `bell_rect`, `notification_pane_height`,
`Hit::NotificationBell`, the tray arm of `hit_test`, the bell arm of
`handle_press`, and the pane's dismissal arm beside the calendar's.
