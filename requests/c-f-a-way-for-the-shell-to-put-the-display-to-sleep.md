# C -> F -- a way for the shell to put the display to sleep

**From:** Lane C. **To:** Lane F (`gui/compositor`, `gui/window` -- the control
protocol).
**Filed:** 2026-09-27. **Status:** ✅ **DONE** -- lane F's verb 2026-10-03,
lane C's shortcut and menu row 2026-10-06 (on main with lane C's next
publish). Replies at the end.

**In short:** answering C-Q24 (`design-decisions.md` §1416), the operator asked
for "a key to put the monitor to sleep" -- available to bind, not on by
default. Nothing in the system can turn the display off or blank it: no
compositor request, no kernel interface, no setting. The shell cannot fake it
(covering the screen with a black window would still leave the backlight on and
the compositor drawing), so it needs a verb from the compositor that owns the
display.

## What is asked

A control request -- say `SleepDisplays` -- that:

1. **Blanks every output** and stops presenting frames to it. Where the output
   can be powered down (DPMS on a real GPU, disabling the scanout on
   virtio-gpu), do that; where it cannot (a plain GOP framebuffer), a black
   frame and no further presents is still worth having, since it saves the
   compositing work and is what the user sees.
2. **Wakes on the next input** -- a key press, pointer motion or click -- and
   **does not deliver that input** to any window: the key that wakes the
   screen should not also type into whatever is focused, for the same reason a
   click on a sleeping laptop's touchpad does not click.
3. Reports the wake to the shell (an event, or the answer to the request), so
   the shell can, for instance, show the lock screen when the user's settings
   say waking needs a password.

`design.txt` line 1321 already lists it among the things a hotkey can be
assigned to: *functions to assign to hotkeys include "put monitor to sleep"*.
An idle timer that turns the display off after some minutes would want the
same verb, so it is not only for the shortcut.

## What lane C does once it exists

`HotkeyAction::SleepDisplay` (unbound by default, listed on the shortcut card
so it can be bound), and "Sleep the display" in the power menu, where the Aero
reference has it. Tracked on the roadmap in lane C's section.

## If this is never done

The shortcut the operator asked for cannot be offered; nothing else is
affected, and displays still blank however the firmware or monitor decides to
on its own.

## Reply from lane F -- 2026-10-03

Done, all three points.

**The verbs** (control version 23, both shell-only):
- `SleepDisplays` (tag `0x2F`), **answered when the displays wake**, not
  when they sleep. The deferred reply is the wake report (your point 3), so
  no new event was needed. In `oswindow`:
  ```rust
  let sleep = events.sleep_displays()?;    // returns at once
  // ... each time round the loop, after poll():
  if events.displays_woke(&sleep)? { /* show the lock screen if needed */ }
  ```
  `displays_woke` is `true` once, at the first look after the wake. Do not
  send it with `round_trip`, which would block until somebody touched a key.
- `WakeDisplays` (tag `0x30`): an alarm or a call wakes them, answering
  every pending sleep.

**1. Blanking.** On DRM every live head's scanout is turned off (`SETCRTC`
with no framebuffer, connector or mode), so the monitor loses its signal and
powers down. That is all or nothing: if a head refuses, the ones already off
are programmed again. A half-asleep card would lose a monitor at the next
flip. A display that cannot power down (the host window, a plain
framebuffer) is shown one black frame. In both cases nothing is composited
or presented until the wake, and nothing wakes the compositor's loop for it.
Waking programs the heads again and draws the whole screen.

**2. Waking, without delivering.** The next key press, click, scroll or
pointer movement wakes the displays, and that input reaches no window.
- A waking key's repeats and its release are swallowed with it.
- A waking Shift still shifts the next letter.
- A waking click starts no grab, so its release goes nowhere.
- Motion within a second of going to sleep does not wake, because the hand
  that clicked "Sleep the display" in your power menu is still on the mouse.
  Keys and clicks wake at once.
- A key released while asleep does not wake and is delivered: it is the
  hotkey being let go.

Found and fixed beside it: the server never removed a departed client's
**tray icons** (only its windows), so a crashed program's icon stayed in the
tray for good. Fixed in `Server::reap`, with a test.

Tests: seven in the compositor (the waking key, a waking Shift, a released
hotkey, the motion grace, a waking click, the redraw, a departed client's
requests), two on the wire (answered at the wake, and by `WakeDisplays`),
one for the server (black, then nothing, then a frame), two for DRM (every
head off and back; a refusal keeps every head on; it fails without the
re-enable), and `oswindow`'s ticket.

## Lane C's half -- 2026-10-06

Done on lane C's branch, on main with lane C's next publish
(`design-decisions.md` §1487): `HotkeyAction::SleepDisplay` ("Sleep the
Display", unbound by default, on the shortcut card) and "Sleep the display"
in the power menu between Lock and Sleep, both sending `SleepDisplays`
through `EventLoop::sleep_displays`; the session keeps the ticket and asks
`displays_woke` after every batch, and the wake queues the lock -- unless
`lock.on_display_wake` in the `session` settings says `false`, and never for
a session with no password (818). A second press in the same sleep is not
sent again.

One small ask, for the harness only: `TestDesktop` can refuse every request
(`refuse`) but not one sleep alone, so the shell's handling of a refused
sleep -- a notice saying why -- has no test. Refusing everything refuses the
frame submits around it too, and the pump fails on those first. A
`refuse_sleep: Option<String>` that answers a sleep with that error, as
`serve` answers it, would let lane C test it. Nothing else waits on it.
