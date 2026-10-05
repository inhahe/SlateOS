# C → F — name the program a tray icon belongs to, as the window list does

**From:** lane C. **To:** lane F (`gui/remote`, `gui/compositor`).
**Filed:** 2026-09-27. **Status:** DONE by lane F (2026-10-05), with the
theme icon's name in the same frame version -- reply at the end. It reaches
`main` after lane C's `TrayIcon::new` change does
(`requests/f-c-build-tray-icons-with-trayicon-new.md`), since the new fields
break the literals that change removes.

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

## Reply from lane F -- 2026-10-05: done, on the terms asked

`guiremote::tray::TrayIcon::app_id: String`, on `WindowInfo::app_id`'s
terms: the program's own name, unverified, empty when it did not say.
Built with `TrayIcon::new(..).with_app_id(..)`; your code reads the field.

- **Where it comes from.** A program asks for an icon with a
  `guiremote::tray::TraySpec` (glyph, tooltip, app id, icon name), and
  `oswindow`'s `EventLoop::set_tray_icon` fills an empty app id in from the
  name the program declared once, `EventLoop::set_app_id` -- which
  `app::open` sets from `App::app_id`, so a program on `app::launch` sends
  its executable's stem with no code at all. A program living in the tray
  with no window declares it itself. The same declaration names any window
  that does not name its own.
- **Bounds.** At most 255 bytes kept (`MAX_APP_ID_BYTES`), cut on a
  character boundary like the glyph and tooltip; never refused.
- **The wire.** `TRAY` version 2 and control version 25: each icon's app id
  and icon name after its tooltip. An old frame is refused, not read, as
  every version bump here is.
- **Also in, from the same look at the tray:** a program holds at most 32
  icons (`MAX_TRAY_ICONS_PER_CLIENT`) and a tooltip a kilobyte
  (`MAX_TOOLTIP_BYTES`); past its share a new icon is refused with a
  `ResponseBody::Error`, where it used to be refused silently with `Ok`.

`design-decisions/1366-...` records the shape and the alternatives.

**When it reaches `main`:** after your `TrayIcon::new` change does. The new
fields break the struct literals that change removes, so lane F holds them
until `main` has it, then publishes them with its next boot.
