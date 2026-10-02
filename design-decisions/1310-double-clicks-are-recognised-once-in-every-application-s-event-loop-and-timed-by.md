## 1310. Double clicks are recognised once, in every application's event loop, and timed by the compositor's clock

**Date:** 2026-09-25
**Lane:** F
**Decided by:** Claude (autonomous), at lane E's request
(`requests/e-f-let-an-application-decline-a-close-so-it-can-ask-about-unsaved-work.md`,
its second ask). The rules for pairing presses are §502's, settled by lane C for
the compositor's title bars and carried over as they stand.

**In short:** double-clicking did nothing anywhere inside an application: the
file picker could not open a file by double-clicking it, and double-clicking a
word selected nothing. Every widget that reacts to a double click was waiting
for an event that nothing produced -- the compositor sends only individual
presses, deliberately, and no application recognised two of them as a pair.
Now every application's event loop does, using the double-click speed the user
set in Settings, and the compositor stamps each press with the time it
happened so that a busy application still pairs clicks by when the user made
them, not by when it got round to reading them.

### What changed

- **`oswindow`**: `EventLoop::poll` recognises a press that completes a double
  click and delivers `MouseEventKind::DoubleClick(button)` straight after it --
  the press itself still delivered, since consumers are written for both. The
  interval is `EventLoop::set_double_click_interval`, which `oswindow::app`
  keeps at `input.yaml`'s `double_click_ms` (the same file and number the
  compositor's title bars use), clamped to the setting's range.
- **`guiremote`**: `InputEvent::time`, the compositor's clock in milliseconds,
  wrapping; input protocol version 7.
- **`compositor`**: stamps every event as it routes it to its client
  (`input_clock_ms`), in the same pass as it handled the input.

### The choices with two sides

1. **In the client's event loop, not the compositor.** The compositor
   deliberately never sends `DoubleClick`: it does not know where a client's
   widgets are, and a pair of clicks on two different rows of a list is two
   clicks. *For the loop:* it sees every press of every application, needs no
   layout, and doing it once there means no application invents its own
   timing. *Against:* the loop pairs on position, not on widgets, so it can
   pair two clicks on neighbouring widgets less than four pixels apart --
   rare, and exactly what every desktop's double click does.
2. **Timed by a stamp from the compositor, which needed a protocol change.**
   Timing the presses as the loop reads them is right while the application
   is responsive, and wrong exactly when it matters: a program busy for a
   second reads two clicks made a second apart one straight after the other,
   and pairs them -- opening a file the user clicked twice slowly because
   nothing seemed to happen. X11, Wayland and Windows all stamp input for this
   reason. *Against:* a wire-format version (6 to 7) and four bytes an event.
   An event nothing stamped (a test's, a synthetic one) is still timed by when
   the loop read it.
3. **Stamped when routed, not when the device reported it.** The server routes
   in the same pass as it handles the input, so the stamp is within a tick of
   the press; stamping at the device would mean threading a time through every
   input source for a precision no double click needs.
4. **§502's rules, plus a distance.** Keyed on window and button; any press in
   between breaks the pair; a completed double click arms nothing, so three
   quick clicks are a double click and a click. And the second press must land
   within four pixels of the first either way (`DOUBLE_CLICK_SLOP`, Windows'
   default), which a title bar did not need and a list does.

### How it is held

`gui/window` tests: two quick presses in one place are followed by exactly one
double click after the second press; 400 ms apart pairs and 401 does not;
clicks stamped 0.6 s apart are not paired though read together; stamps compare
across the clock's wrap; two windows, two buttons, a press in between, or a
move past the slop each break the pair; three clicks are one double, four are
two; unstamped presses pair when read together and not when read further apart
than the interval (a sleep that can only overrun); and the interval in
`input.yaml` reaches the loop. `guiremote`: a stamp round-trips, zero included,
and a bad presence byte is refused. `compositor`: routed events carry a clock
that moves with time. Letting a completed double click re-arm, or timing
stamped presses by when they were read, each fails the suite.
