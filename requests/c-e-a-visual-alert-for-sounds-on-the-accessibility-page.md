# C → E — a visual alert for sounds, on the accessibility page

**From:** lane C. **To:** lane E (`apps/settings`).
**Filed:** 2026-10-07. **Status:** OPEN.

## In short

The desktop can now show its sounds as well as play them: when it makes a
sound -- a notification's chime, a device plugged in, the volume changing --
it can flash the window in front or the whole screen, for someone who cannot
hear it (`design-decisions/1603-a-sound-the-desktop-makes-can-be-seen-as-a-flash.md`).
It is off until chosen, and so far the only way to choose is to edit
`appearance.yaml` by hand. The Settings application's accessibility page is
where it belongs -- under hearing, where Windows keeps its "visual
notifications for sounds".

## What is asked

One choice on the accessibility page that sets
`appearance::AppearanceSettings::sounds.visual_alert`:

- **The value:** `appearance::VisualAlert` -- `Off` (the default), `Window`,
  `Screen` -- each with its label (`VisualAlert::label`: "Off", "Flash the
  active window", "Flash the screen"), every one in `VisualAlert::ALL`, in
  the order a list shows them. Saved as the page saves the rest of
  `appearance.yaml` (`sounds:` / `visual_alert:` -- `AppearanceSettings::write_into`
  already writes it); the desktop follows the file as it is saved, as it
  follows the rest of the appearance settings.
- **The control:** three radio buttons, or a drop-down; the toolkit has both
  (`guitk::widget`).
- **A line saying what it does:** for example "When the desktop makes a
  sound, flash the screen too -- for when you cannot hear it." And, since
  the flash stands for a sound, that an event whose sound is off flashes
  nothing.

## Nothing else needed

The desktop reads the setting, flashes, and spaces the flashes; the page only
chooses. Reply here when it lands.
