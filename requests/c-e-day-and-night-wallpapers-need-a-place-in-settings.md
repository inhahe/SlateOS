# C → E — day and night wallpapers work; the Wallpaper page has nowhere to choose them

**From:** lane C. **To:** lane E. **Filed:** 2026-09-27.
**Status:** DONE 2026-09-29 (lane E, 3a7073848). `settings --page wallpaper`,
"By time of day", between the picture and the rotation: "Daytime picture"
and "Evening picture", each with the picture chooser, set up from 06:00
and 18:00; an "Up from" dropdown per picture moves it, keeping the morning
the earlier (a lone picture keeps to its half of the day); Clear stops it.
While one is set the section says so and which picture is up now, by the
machine's zone, and the picture and the rotation each say they are not
shown. A hand-written schedule of more than two is listed as it is, with
Clear. No switch: choosing a picture turns it on and Clear off, so there is
no "on" with nothing to show.

Original status: open — a control on a page; nothing is blocked on it, and the
feature works meanwhile for anyone who edits `appearance.yaml`.

## In short

The desktop can change its wallpaper by the time of day now: a day picture
from 06:00, a night picture from 18:00, or any number of pictures at any
times (`roadmap-detailed.md` §3.4, "dynamic wallpapers"). It is read from
`appearance.yaml` like every other wallpaper setting, and the desktop
changes the picture at each time without being touched. What is missing is a
way to set it that is not a text editor: this asks the Settings application's
Wallpaper page for one.

## What there is

`appearance::AppearanceSettings::wallpaper_schedule: Vec<ScheduledWallpaper>`,
each `ScheduledWallpaper { from: TimeOfDay, image: PathBuf }`, kept sorted by
time. In the file it is `wallpaper.schedule`, one `"HH:MM path"` per entry,
the path percent-encoded as `wallpaper.image` is (the page never needs to
know: `AppearanceFile::save` writes it).

- Each picture is up from its time until the next entry's; before the day's
  first entry, the last one is still up from the evening before.
- A schedule wins over a rotation folder and over a single picture, since it
  *is* the wallpaper -- the page should make that visible rather than let a
  user choose a picture that a schedule then hides.
- `AppearanceSettings::scheduled_wallpaper_at(utc_secs, zone)` answers which
  picture is up at a moment, for a preview ("up now: night.jpg").

## What the page might offer

The smallest useful form is two rows, "Daytime picture from [06:00]" and
"Evening picture from [18:00]", each with the page's existing picture chooser,
and a switch that turns the pair on. The general list (add a time, pick a
picture) can come later; the file already holds it.

Saving is `AppearanceFile::load` / edit / `save` as the page does now; the
desktop picks the change up from the announcement it already gets.
