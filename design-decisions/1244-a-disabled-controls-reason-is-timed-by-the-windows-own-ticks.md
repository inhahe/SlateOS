## 1244. A disabled control's reason is timed by the window's own ticks

**Date:** 2026-10-10 · **Decided by:** Claude (operator-approved scope: lane
C's request `requests/c-e-say-why-a-control-is-disabled.md` asked every
program to say why a disabled control is disabled, through the toolkit's
`WhyDisabled` and `ContextMenu::explain`, and left how each program keeps
time to lane E) · **Lane:** E

**In short:** A greyed button now says why it is greyed when the pointer
rests on it for half a second. "Half a second" needs a clock, and a window
program on SlateOS has two it could use: the computer's real clock, or the
ticks the window system sends it (each tick says how long it has been since
the last one). Every program lane E has given a reason to -- Settings,
thirteen games, the lock screen, System Restore, three context menus --
counts the ticks: the clock is the sum of the ticks' elapsed times, and the
program asks for a tick only while a reason is waiting to appear. The one
thing this gets slightly wrong: moving straight from one greyed button to
another before the first's reason appeared, the second's can appear up to
half a second early. It is never late.

### What was chosen

The window keeps `clock_ms`, the sum of every `Event::Tick`'s `elapsed_ms`,
and hands it to the toolkit as "now". `tick_interval` asks for a tick
exactly when a reason is waiting (`WhyDisabled::due_in`, `ContextMenu::due_in`),
alongside whatever ticks the program wanted already, and not otherwise -- so
a window nobody points at sleeps. `gamechrome::why::Reasons` packages this
for the games; Settings, the lock screen and System Restore hold a
`WhyDisabled` and a clock themselves; the context menus are ticked by the
program that owns them.

### The alternatives

| | For | Against |
|---|---|---|
| **The ticks' own clock** (chosen) | Tests drive time exactly, with `Event::Tick { elapsed_ms }`, as they drive every other animation in the tree -- no sleeping, no flakiness. One clock for everything a window times. | It stands still between waits, so a wait started while another runs begins at the stale reading: early by up to the delay, in that one case. |
| The real clock (`Instant`) | Exact. | A test would have to sleep half a second per reason, or the program would need a clock injected for tests -- two clocks, one of them untested in production. |
| A timestamp on every event | Exact, and testable. | The window protocol's events carry no time; adding one is a change to every program and lane C's and lane F's protocol, for a tooltip's half second. |

### What would change it

A timestamp on the window protocol's events, for some reason that matters
more than this, would make the third row free; then a pointer event's own
time would start the wait and the early case would go.
