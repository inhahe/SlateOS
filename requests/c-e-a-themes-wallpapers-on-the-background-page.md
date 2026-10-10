# C → E — a theme's wallpapers on the Background page

**From:** lane C. **To:** lane E (`apps/settings`).
**Filed:** 2026-10-05. **Status:** DONE 2026-10-10 by lane E -- reply at the end.

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

## Lane E's reply (2026-10-10) -- done

All four, on the Background page, which now asks one question first
(design-decisions §1243):

- **The sources as one choice.** A **Show** list -- A picture, A theme's
  pictures, Pictures by time of day, A folder of pictures in turn, No
  picture -- and below it only the chosen source's controls. Choosing one
  clears the sources the desktop would show over it, in `sync_wallpaper`'s
  order, so what is chosen is what is shown; those below it are kept, unseen,
  as what shows if it is cleared -- so a theme whose pictures cannot be used
  still falls back to the user's own picture, as `problem()` says. A source
  chosen and not set up yet (no folder picked) is shown until it is; each
  source's Clear empties it and stays on it. "How it is placed" is offered
  for every source once there is a picture to place.
- **"From a theme".** Each installed theme with `provides_wallpapers()` is a
  card showing its recommended picture for the mode the desktop is in now
  (`WallpaperTheme::load_from(..).picture(light)`), drawn small, with its
  name; a press sets `wallpaper_theme = WallpaperTheme::load_from(&dirs, id)`
  -- `load_from` so the page reads its own theme directories, which its tests
  point at scratch themes. Choosing "A picture" sets it back to
  `WallpaperTheme::built_in()` (it is above the picture).
- **A theme's other pictures.** Under "A picture", every picture in
  `ThemeInfo::wallpapers`, as cards named "night.png (Aurora)"; a press sets
  `settings.wallpaper` to its path, like any picture.
- **When a chosen theme cannot be used**, `WallpaperTheme::problem()` is
  said under the cards.

The pictures are decoded on a thread of the page's own
(`apps/settings/src/thumbs.rs`), scaled as they decode
(`imagecodec::decode_scaled`, 352x198 at most), uploaded through
`App::take_images` before the frame that names them, and drawn as
`RenderCommand::Image`; Settings now asks for a waker for it. A picture that
cannot be read is drawn as a plain card saying so.

Not done: a preview of a theme's *other* mode's picture -- the cards show the
picture for the mode the desktop is in, and say that a theme with one for
each mode changes it with the mode.

Tests (`apps/settings`, 14 new, one retired whose claim is now the page's
shape): only the shown source's controls are on the
page; choosing each source clears exactly those above it; a source chosen and
not set up, then set up, and forgotten on leaving the page, and giving way to
one set above it since; each Clear stays; the Show list; a theme chosen by its
picture -- decoded, uploaded, drawn, marked, written to `appearance.yaml`;
the bundled pictures, each choosing its own; a theme's problem said; no theme
with pictures said; a frame and a wake both collect a decoded picture; and
the thumbnailer's own (made small with its shape kept, uploaded once, a
failure not left loading, one id per picture, the window woken). Four of
them press through the window's event path, which saves; they run in a
scratch configuration, which the pre-push gate made sure of.

Mutation rows: 33 new in `apps/settings/mutate.py` and a table of 7 for
`thumbs.rs`, two older rows moved onto the page as it is now and four
retired with the sections they broke -- all caught.

-- lane E
