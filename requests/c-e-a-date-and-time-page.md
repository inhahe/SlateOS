# C → E — A Date & Time page in the Settings app

**From:** Lane C (`gui/datetimesettings`). **To:** Lane E (`apps/settings`).
**Filed:** 2026-09-25. **Status:** OPEN -- lane C's half is done.

**In short:** the desktop's clock settings now have a file and a shared model
(`gui/datetimesettings`, `design-decisions.md` §875), and the shell obeys it:
the taskbar clock's time zone, whether it shows seconds, the weekday and the
date, and up to four world clocks shown in the calendar. Nothing in the
running system can change any of them yet, because the Settings app has no
Date & Time page. Until one exists, the only way to set your time zone is to
edit `~/.config/slateos/datetime.yaml` by hand.

## The API

- `datetimesettings::DateTimeFile::load()`, change `.settings`, `.save()` --
  the same shape as `notifsettings::NotifFile`. The save splices into the
  user's file, keeping their comments.
- **The zone.** `settings.set_zone(Some(tz_id))` for one of
  `datetimesettings::zones()` (21 zones, UTC first, each with `display_name`,
  `city`, `offset_string(now)` and `is_dst_at(now)` for the list), or
  `set_zone(None)` for **the machine's own zone** -- the default, and worth a
  row of its own ("Automatic (this computer's zone)"): it is what the libc and
  `date` use. `search_zones(query)` matches name, description and city.
- **The taskbar clock.** `show_seconds`, `show_day_of_week`, `show_date`.
- **World clocks.** `add_clock(tz_id, label)` refuses a fifth, an unknown zone,
  an empty label and a label already used (the file keys clocks by label);
  `remove_clock(index)`; each clock's `visible` hides it without losing it.

## Two things the page should know

- **A save does not reach a running shell yet.** The relay that tells the shell
  to re-read the file is lane F's to add (`requests/c-f-a-settings-group-for-the-date-and-time.md`);
  until it lands, the shell reads the file when it starts. When it lands, send
  its request after each save, as the notification page does with
  `ReloadNotifications`.
- **A design reference exists.** `gui/desktop/src/datetime_settings.rs` has a
  complete panel for exactly this -- tabs for the date and time, the zone
  picker with a search field and a DST badge, and the world clocks -- which the
  shell never puts on screen (`known-issues.md`
  `TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES`; by §815 a page you
  open lives in Settings). Its NTP tab is not a model to follow: nothing obeys
  those settings, which is why they are not in `datetime.yaml`.

## What happens until it is done

The clock shows the machine's own zone (UTC on a machine with no
`/etc/localtime`), with the date and weekday, and changes only by hand-editing
the file.
