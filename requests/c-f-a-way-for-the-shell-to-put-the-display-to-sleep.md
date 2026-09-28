# C -> F -- a way for the shell to put the display to sleep

**From:** Lane C. **To:** Lane F (`gui/compositor`, `gui/window` -- the control
protocol).
**Filed:** 2026-09-27. **Status:** OPEN -- a protocol verb; lane C wires the
shortcut and the menu entry once it exists.

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
