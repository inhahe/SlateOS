## TD-C-THE-POWER-AND-MONITOR-MANAGERS-ARE-CONSTRUCTED-BY-NOTHING (lane C, 2026-10-17)

**Status:** OPEN -- found, not yet triaged.

**In short:** two subsystems of the desktop shell are written, tested and
never built by the running shell: the power manager (what to do when the lid
closes, the power button is pressed or the machine sits idle; power
profiles; a screen saver; low-battery actions) and the monitor manager
(arranging several displays, their resolution, rotation and scale, saved
across restarts). Nothing a user can do reaches either. Neither is wrong;
each is unused, and some of what they model is done elsewhere now.

**How it was found.** `scripts/check-tested-but-uncalled.py --all` on lane
C's trees (1140 functions called only by their own tests) and a look at the
modules with the most. Neither shows in `scripts/scan-orphan-modules.py`:

| Module | Lines | What is used from it | Why the orphan scan misses it |
|---|---|---|---|
| `gui/desktop/src/multimon.rs` | 1999 | nothing -- `pub mod multimon;` is its only mention | `widgets.rs` declares a `MonitorLayout` of its own and several apps a `Rotation`, and the scan clears a module on any shared name |
| `gui/desktop/src/power.rs` | 3074 | `PowerChoice` and the power menu's rows (wired: the start menu's power menu runs `powerctl`); `BatteryInfo`, always its `Default` (no battery source, below) | the used half clears the module |

**What each would need, and what already does part of it** -- the triage
question `TD-C-THREE-MORE-SHELL-SETTINGS-MODULES-ARE-REACHED-BY-NOTHING`
settled on: delete what a working implementation elsewhere replaces, keep
what holds design recorded nowhere else.

- **`PowerManager`** (`on_lid_close`, `on_power_button`, `on_sleep_button`,
  `on_user_activity`, inhibitors, transitions) -- *idle* is done by the
  compositor (`EventLoop::watch_idle`, `idle_lock.rs`), so its own idle
  counter is a second model of that. *Lid and buttons* have no source: the
  kernel delivers no ACPI power-button or lid event to the shell (lane A).
  *Battery*: `kernel/src/fs/battery.rs` has no source either (no ACPI driver
  calls `register_source`), which `lib.rs`'s `BatteryInfo::default()` line
  already says. *Inhibitors* ("a video is playing, do not sleep") have no
  caller and no protocol for a program to ask.
- **`ScreenSaver`** (blank, clock, matrix rain, starfield) -- no screen saver
  exists in the shell; "Sleep the display" is the compositor's.
- **`MonitorManager`, `MonitorConfig`, `WindowPlacement`** -- the
  compositor owns the outputs (lane F); whether a display arrangement the
  user chooses is lane F's to keep, or the shell's to keep and hand over, is
  the question to put to lane F before keeping any of it.

**What the spec says.** `design.txt`'s "power options" asks for an action on
the power button, an action on closing the laptop, the screen off and the
computer asleep after N minutes, and an action on low battery -- which is
`PowerManager`'s model, so that is specified design, not leftovers. A
screen saver is not in it.

**The proper fix:** triage each as the sibling entry did. Likely outcome,
to confirm: `multimon.rs` goes if lane F's compositor keeps the display
arrangement (asked: `requests/c-f-who-keeps-the-users-display-arrangement.md`);
`PowerManager`'s own idle counter goes, the compositor's `watch_idle` being
the idle source, and the rest of it is wired -- the screen-off delay on
`watch_idle` (done, below), the sleep delay likewise, the lid, buttons and battery the day lane A
delivers those events (a request to lane A for a power-event channel); the
screen saver, unspecified, goes unless the operator wants one.

**The screen-off delay is done (2026-10-17)**, without `PowerManager`:
`EventLoop::watch_idle(window, after)` holds one delay per window, and an
event reaches the session with the window it was told to
(`ShellSession::dispatch`), so the displays' delay is claimed on the
background's window beside the lock's on the panel. `idle_lock::displays_off_after`
reads `displays.off_after_minutes` in the `session` group, and the idle
event for the background puts the displays to sleep as "Sleep the display"
does. The Settings page that writes it is asked of lane E
(`requests/c-e-settings-offers-turning-the-screen-off-after-n-minutes.md`).
"Put the computer to sleep after N minutes" can take the same shape on a
third window, once the shell can ask `powerctl` to suspend from idle (the
power menu's Sleep already runs it).

**If never fixed:** 5,000 lines that read as features the desktop has, and
a battery icon that can only ever say there is no battery.
