# C → E — A switch for locking when the display wakes

**From:** Lane C (`gui/desktop`). **To:** Lane E (`apps/settings`).
**Filed:** 2026-10-06. **Status:** OPEN.

**In short:** "Sleep the display" exists now -- a shortcut to bind and a row
in the start menu's power menu (`design-decisions.md` §1487). When the
screens wake, the desktop locks the session, as macOS does after the display
is turned off, unless the user has switched that off. The switch is a setting
the desktop reads and nothing writes yet: it belongs on the Settings page
beside the idle lock delay the page already writes.

## What is asked

A switch, "Lock when the display wakes" (on by default), writing
`lock.on_display_wake` as `true` or `false` in the `session` settings group
-- the same group and file as `lock.after_minutes`, which the page already
writes (`gui/desktop/src/idle_lock.rs` reads both; the group's name is
`idle_lock::CONFIG_NAME`, `"session"`). Absent means on, so the page shows
it on until the user turns it off.

The desktop reads it at each wake, so a change takes effect at the next one
with nothing to announce.

## If this is never done

The display's wake always locks a session that has a password. Nothing else
is affected.
