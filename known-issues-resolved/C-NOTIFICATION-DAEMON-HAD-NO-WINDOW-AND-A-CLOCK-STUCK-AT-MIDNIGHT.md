## C-NOTIFICATION-DAEMON-HAD-NO-WINDOW-AND-A-CLOCK-STUCK-AT-MIDNIGHT (lane C, 2026-08-26) — ✅ FIXED 2026-08-26

**In short:** the notification daemon — the thing that pops up "battery low",
"download finished", "new mail" — never opened a window, so none of it ran. And
underneath that was a worse bug: the daemon has a "quiet hours" feature (don't
interrupt me between 22:00 and 07:00), and it had no clock. Its idea of the
current time was fixed at midnight, forever, because the one method that sets
the time had no caller anywhere in the codebase. Midnight is inside 22:00–07:00,
so quiet hours were permanently on, and every notification that was not marked
Critical was silently thrown away. The user would have seen a notification
system that never notified. Both are fixed: the daemon now opens a window and
reads the wall clock every second.

### What changed

1. **The window.** `gui/notifications` gained `oswindow` and an
   `impl oswindow::app::App for NotificationDaemon`. `fn main` used to build a
   daemon, install a quiet-hours schedule on it, and drop it (`let _ = daemon;`);
   it now calls `oswindow::app::launch("notifications", &mut daemon)`.
   `render(&mut self, width, height)` calls `set_viewport(width, height)` before
   rendering, so the overlay believes the size it is *granted* rather than the
   1920×1080 it asks for — toasts are placed against the right edge, so a daemon
   that kept believing in 1920 on an 800-wide screen would draw them off the side
   *and* hit-test their close buttons there.

2. **The clock.** New `set_time_from_utc(i64)` (pure, assertable) and
   `refresh_clock()` (reads `SystemTime::now`), mirroring the split the desktop
   clock and the lock screen already use. The zone is an explicit
   `tzrules::Tz::utc()` read through `lookup(t).gmtoff`, not `secs % 86_400`, so
   `rg 'Tz::utc' apps/ gui/` still finds every surface that renders an instant.
   `rem_euclid`, not `%`, so a pre-1970 instant is a time of day rather than a
   negative number that would clamp back to midnight — i.e. back into quiet
   hours, which is the exact failure this is about. `refresh_clock` is called
   from the `Event::Tick` arm of `on_event` and once more before the first frame,
   so the daemon never answers a quiet-hours question from its seeded midnight.

3. **The tick cadence.** `tick_interval` is never `None`: 16 ms while a toast is
   on screen (it animates and ages), one second otherwise. One second and not one
   minute even though the clock is read in whole minutes — a 60-second timer
   started at an arbitrary moment turns the minute over up to 59 seconds late, so
   quiet hours would begin somewhere in the minute *after* 22:00. Returning
   `None` when the last toast leaves would have been the trap: the daemon would
   never see 22:00 arrive, and having asked for no more ticks, would never get
   another chance to.

4. **`Idle` versus `Redraw`.** `handle_event` returns `Option<DaemonAction>`, and
   an action is *not* the same question as "did the display move". Three events
   move it without producing one — a tick (toasts age; the centre's "2 minutes
   ago" labels move with `current_time_ms`), a resize, and a scroll of the centre
   — and everything else that moves it does return an action. What is left (a
   mouse move, an unbound key, a focus change) leaves the tree byte-identical, so
   it maps to `Idle` rather than asking the compositor to redraw the same pixels
   sixty times a second while the pointer crosses the screen. The condition is
   sampled *before* the event is handled, because the tick that removes the last
   toast still has to be drawn — there is a test named for exactly that.

### The bug this nearly kept, and how it hid

`set_time_of_day` was correct, public, documented, and covered: seven tests call
it. Not one of them was a *product* test — they all set the time and then asked
about DND, which is the same shape as the lock screen's `has_password` hazard
recorded above. The tell is the same one and is worth looking for again: **a
method that only tests call is a method production does not call.** Grep for the
setter, not the getter.

`scripts/check-tick-wiring.py` did not catch it either, and could not: the daemon
*does* route `Event::Tick`, straight into `tick(delta_ms)`, which advances the
monotonic animation clock. There were two clocks here — a monotonic one that was
wired and a wall-clock one that was not — and a gate that asks "is there a tick
arm?" sees the first and is satisfied. The gate is not wrong; it measures what it
says it measures.

### Cost

- 47 → 59 tests in the crate; new deps `oswindow`, `tzrules`.
- `scripts/check-window-wiring.py` `BASELINE` 128 → 127.
- Two of the twelve new tests failed on their first run, both usefully: a toast
  measured immediately after arrival is still off the right edge mid-slide-in
  (so the size test measures the animation, not the placement), and expiry takes
  two ticks — one to mark the toast dismissing, one to finish its exit animation.

### What is still owed

- **The daemon has no IPC endpoint.** `handle_request` is the whole service API
  and nothing calls it in production; notifications can only arrive from inside
  the process. Wiring it needs the channel-IPC service registration that lane A
  owns, and is the same blocker as `requests/c-a-*`. The window is a
  prerequisite, not a substitute.
- **Quiet hours are UTC**, like every other lane-C clock, because no per-process
  zone exists (`TD-NO-SYSTEM-DEFAULT-ZONE-WITHOUT-TZ`). A user whose day is not
  UTC gets quiet hours at the wrong time of day. That is one fix in one place
  once the zone exists, and `Tz::utc()` is greppable so the place is findable.
- **The 22:00–07:00 default is hardcoded in `main`** and cannot be changed
  without a rebuild. It belongs in the settings store next to the rest of the
  user's preferences.

### The lesson

**Two clocks in one struct is two clocks to wire, and wiring one of them makes
the gate green.** `current_time_ms` (monotonic, for animation) and
`current_minutes` (wall clock, for quiet hours) are different quantities that
both look like "the time"; the tick arm advanced the first and the second sat at
its initial value for the life of the process. When a struct holds more than one
notion of "now", check each one's setter for a production caller separately — a
passing tick-wiring check tells you about the arm, not about everything the arm
should be doing.
