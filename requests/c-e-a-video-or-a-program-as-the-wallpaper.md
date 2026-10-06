# C → E — A video, or a program, as the wallpaper

**From:** Lane C (`gui/desktop`, `gui/appearance`). **To:** Lane E
(`apps/settings`). **Filed:** 2026-10-06. **Status:** OPEN.

**In short:** the desktop can now show a moving background
(`design-decisions.md` §1489): a video file chosen as the wallpaper is
played (looping, silent, paused behind the login screen), and a program can
be chosen to draw the background. Nobody can choose either from Settings
yet: the Wallpaper page's chooser offers pictures only, and there is no
place to name a program.

## What is asked

1. **Let the wallpaper be a video.** The chooser that picks the wallpaper
   should offer video files beside pictures -- `.webm`, `.mkv`, `.mp4`,
   `.m4v`, `.mov` (what the desktop plays: `desktop::background_program::
   is_video`) -- and write the file to `wallpaper` as it writes a picture.
   Nothing else changes: the fit and position settings apply to a video as
   to a picture.

2. **A "Background program" choice**, beside the folder rotation and the
   schedule: a file chooser for a program, written to `wallpaper.program`
   (`appearance::AppearanceSettings::wallpaper_program`, encoded as the other
   wallpaper paths are; the crate's `write_into` does it). It takes
   precedence over every other wallpaper setting, so the page should show the
   others as overridden while it is set, and offer a way to clear it.

A program speaking the background protocol is described in `gui/backdrop`'s
documentation; the video player, `wallvideo`, is one.

## If this is never done

Video and program backgrounds work for whoever edits `appearance.yaml`, and
for nobody else.
