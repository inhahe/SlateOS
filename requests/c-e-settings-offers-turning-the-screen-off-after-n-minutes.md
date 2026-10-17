# C → E: the Settings app offers "Turn off the screen after N minutes"

**From:** lane C. **To:** lane E (`apps/settings`). **Filed:** 2026-10-17.
**Status:** OPEN. **Priority:** low.

## In short

`design.txt`'s power options ask for "turn off screen after N minutes
(disablable)". The shell does it now (lane-c-preview, the commit that adds
`idle_lock::displays_off_after`): it reads a delay in whole minutes from the
`session` settings group, key `displays.off_after_minutes`, asks the
compositor to say when the session has been idle that long, and then puts
the displays to sleep exactly as "Sleep the display" does -- so waking them
locks the session if `lock.on_display_wake` says so. A change is adopted
without a restart (the `Session` settings announcement re-claims the watch).

Nothing writes the key yet. The lock screen page (`apps/settings/src/lockscreen.rs`)
writes its neighbour, `lock.after_minutes`, and is the natural place for a
second control beside it.

## What the shell reads

| | |
|---|---|
| group | `session` (`idle_lock::CONFIG_NAME`) |
| keys | `displays` → `off_after_minutes`, and (added the same day, design.txt's "put computer to sleep after N minutes") `sleep` → `after_minutes`; integers |
| meaning | minutes of no input before the displays sleep; before the computer sleeps, as the power menu's Sleep puts it |
| never | `0`, absent, negative or not a number -- the same rule as `lock.after_minutes`: nought is "never", not "at once" |

Announce `SettingsGroup::Session` after writing, as the lock delay's page
does.

## Worth saying on the page

The two delays work together: with the screen off after 5 minutes and the
lock after 15, the displays sleep first, and waking them locks only if
"Lock when the displays wake" is on. A shorter screen-off than lock is the
usual choice; the page may want to say so rather than enforce it.
