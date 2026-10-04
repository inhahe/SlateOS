# C → F — name the program a tray icon belongs to, as the window list does

**From:** lane C. **To:** lane F (`gui/remote`, `gui/compositor`).
**Filed:** 2026-09-27. **Status:** OPEN -- accepted by lane F (2026-10-03); the
field waits on lane C building icons with `TrayIcon::new`
(`requests/f-c-build-tray-icons-with-trayicon-new.md`), because adding it
now would break seven literals in `gui/desktop`'s tests.

## In short

`roadmap-detailed.md` §3.4 asks that a user can drag icons into and out of the
system tray -- keep a program's icon always visible, or always behind the
"Show hidden icons" chevron -- and that a program can be set to always start
in the tray. Both are choices a user makes about a *program*, and they must
outlast a restart. A tray icon today carries its owner as a process id
(`guiremote::tray::TrayIcon::owner`), which is a new number every time the
program starts, so the shell has nothing to remember the choice by.
`design-decisions.md` §844 said as much: if tray icons become something a user
arranges deliberately, this becomes a setting -- which needs a name for the
program.

## What is asked

An `app_id` on `TrayIcon`, on the same terms as `WindowInfo::app_id` in
`window_list.rs`: the program's own declared name (conventionally its
executable's stem, lower-cased), client-supplied and so unverified, fine for
cosmetic grouping and never a permission. Empty when the program did not say.

- The natural source is whatever the client already declares for its windows;
  a program with a tray icon and no window would declare it with the icon.
- On the wire, one more length-prefixed string in the `TRAY` frame, under a
  new frame version (a version-1 frame decodes with an empty `app_id`).
- Nothing else changes: the owner stays the pid the compositor fills in, and
  clicks still go by owner and id.

This could land in the same frame version as the icon *name* proposed in
`requests/c-f-let-a-tray-icon-name-a-theme-icon.md`, if that is convenient --
they are independent fields.

## What lane C does with it

Keep a per-program tray arrangement (shown, hidden, and the order) in the
shell's settings, keyed by `app_id`; let an icon be dragged onto the chevron to
hide it and out of the overflow list to show it; and remember it across
restarts. Icons with an empty `app_id` keep today's behaviour -- shown in
registration order, overflowing by space.

## If this is never done

The tray works as it does now: every program's icon is shown while there is
room, and a user cannot choose which.
