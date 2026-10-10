# C → E — a theme's wallpapers on the Background page

**From:** lane C. **To:** lane E (`apps/settings`).
**Filed:** 2026-10-05. **Status:** OPEN.

## In short

A theme can now bundle desktop pictures and recommend one for dark mode and
one for light (`design-decisions/1471-…`). The desktop shows the chosen
theme's picture for the mode it is drawn in, so a day picture and a night
picture follow the automatic mode. It is chosen as the other parts of a
theme are, by `theme.wallpaper: <name>` in `appearance.yaml` -- and nothing
in Settings can set that yet.

## What is asked

On the Background (wallpaper) page:

- **"From a theme"** as a source of the wallpaper, beside a picture, a
  rotating folder and a time-of-day schedule: a list of the installed themes
  for which `appearance::themes::ThemeInfo::provides_wallpapers()` is true,
  each shown by its recommended picture
  (`AppearanceSettings::wallpaper_theme` after choosing it, or
  `WallpaperTheme::load(id).picture(light)` to preview). Choosing one sets
  `settings.wallpaper_theme = WallpaperTheme::load(id)`; choosing a picture
  of one's own sets it back to `WallpaperTheme::built_in()`.
- **The sources as one choice.** The desktop's order is: a schedule, then a
  rotating folder, then the theme's picture, then the fixed picture
  (`gui/desktop/src/session.rs`, `sync_wallpaper`). A page that lets two be
  set at once shows one and not the other with no word why -- so choosing
  one source should clear the others, or the page should say which is shown.
- **A theme's other pictures** -- `ThemeInfo::wallpapers`, every picture its
  `wallpapers/` folder bundles -- offered where a picture is chosen, as
  pictures like any other (a path in `settings.wallpaper`).
- **When a chosen theme cannot be used** -- not installed, or recommending
  no picture it has -- `WallpaperTheme::problem()` says why in a sentence;
  the desktop shows the user's own picture meanwhile and says nothing on
  screen (as for every axis but the colours).

## If it is never done

Nothing breaks: the desktop shows the user's own picture, as before, unless
`theme.wallpaper` is written by hand.
