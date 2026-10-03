## 876. "System (Auto)" follows fixed light hours, 07:00 to 19:00 by default, and every program learns of the edge through the settings watcher

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The appearance settings have offered "System (Auto)" beside
Light and Dark since they were written, and it has always meant "dark": nothing
decided when it should be light. It now follows a daily schedule -- light from
07:00 until 19:00 unless the user sets other hours -- in the clock's time zone
(the user's choice in `datetime.yaml`, or the machine's). The desktop, the
window frames and every application switch together at the edge, without the
settings file changing.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| What decides light or dark? | fixed hours the user can set, `theme.auto.light_from` / `dark_from` | sunrise and sunset | the machine has no location to compute a sunset from, and asking for one to switch a colour scheme is a privacy cost with no other use yet. Fixed hours are also what a user who works nights can set. Sunset remains an option to add beside them. |
| The default hours? | 07:00 to 19:00 | follow the day's actual length | roughly the working day and the hours a room is most often daylit; a default is a starting point, and the hours are one setting away. |
| Where is "light now?" decided? | when the settings are read (`read_from`), into `auto_is_light` | per frame | the answer needs the clock and a zone file; per frame is where no file may be read. Every reader resolves it for itself, so no program needs another to tell it the phase -- only to look. |
| How does a program learn to look at the edge? | the appearance watcher's fingerprint includes the phase, and the shell, which sleeps until the edge, sends `ReloadAppearance` | write the phase into `appearance.yaml` | the file is the user's preferences; a phase written into it by the shell would be clobbered by the Settings app's next save of its older copy, and would put runtime state where only choices belong. |
| Which clock? | `datetimesettings::clock`, one wall clock for the desktop, with a seam a test can fix | `SystemTime::now()` at each site | a test of anything scheduled by the time of day otherwise passes by day and fails by night; and the shell had two copies of the same function. |
| And "is it light?" | one answer, `AppearanceSettings::is_light`, which the palette and the accent follow | `ThemeMode::is_light` | the setting alone cannot say: the schedule decides `System`, and a one-mode colour theme decides for itself (§874). Asking the mode put a light-background accent on a dark-only theme -- fixed first, in its own commit. |

### What is not done here

- Applications switch at the edge once lane F's `ThemeWatch` watches through
  `appearance::watcher()` (`requests/c-f-watch-appearance-through-appearance-watcher.md`);
  until then they switch at the next announcement that finds the settings
  changed. The shell and the window frames switch at the edge now.
- The Settings page for the hours is lane E's (`requests/c-e-the-automatic-modes-hours.md`).
- A system-wide quick toggle between light and dark -- the roadmap's "or
  system toggle" -- is not built; the mode is chosen in Settings.
