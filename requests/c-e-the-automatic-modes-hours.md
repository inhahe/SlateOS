# C → E — The automatic mode's hours, beside "System (Auto)" in Settings

**From:** Lane C (`gui/appearance`). **To:** Lane E (`apps/settings`).
**Filed:** 2026-09-25. **Status:** OPEN -- lane C's half is done.

**In short:** "System (Auto)" now does something: light from 07:00 until 19:00
and dark the rest of the day, in the clock's time zone (`design-decisions.md`
§876). The hours are a setting -- `AppearanceSettings::auto_light_hours`, a
`daywindow::DailyWindow` -- with nowhere in Settings to change them. The
Appearance page's Theme Mode cards (`apps/settings/src/main.rs`, about line
3866) are where they belong: two times, shown when "System (Auto)" is chosen.

## The API

- `settings.auto_light_hours`: `DailyWindow::new(light_from, dark_from)`;
  `start()` / `end()` are `TimeOfDay`s, and `TimeOfDay`'s `Display` and
  `TimeOfDay::parse` are the `HH:MM` spelling the file uses -- the same one
  the quiet-hours page shows through `notifsettings::format_hm`, which now
  delegates to it. A window whose two times are equal never changes; worth
  refusing, or saying "always dark".
- `settings.is_light()` is whether what is drawn is light *now* -- the
  schedule for `System`, and a one-mode colour theme's own mode. **Use it
  instead of `theme_mode.is_light()`**, which the page does at about line
  3933 to choose the accent swatches: that asks the setting, not the screen,
  and the two now differ at night.
- After saving, `appearance_changed()` as for any appearance setting.

## What happens until it is done

The automatic mode uses 07:00 to 19:00; other hours can be set by hand under
`theme: auto:` in `~/.config/slateos/appearance.yaml`
(`light_from: "06:30"`, `dark_from: "20:00"`).
