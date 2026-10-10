# C -> E: choose the cursor theme in Settings

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-01.
**Status:** OPEN -- the setting works without it (a hand edit of
`appearance.yaml`), and nothing draws from it until lane F's half lands
(`requests/c-f-draw-the-pointer-from-the-cursor-theme.md`).

**In short:** A user can choose a cursor theme -- the pictures the mouse
pointer is drawn as -- from the cursor themes installed, which includes any
theme made for GNOME or KDE (Adwaita, Breeze, Bibata ...). Settings needs a
chooser for it, beside the pointer size and colour it already has, or on the
theme page beside the icon theme. `design-decisions.md` §1459.

## What there is

- The setting: `theme.cursors` in `appearance.yaml`, a theme's folder name;
  `AppearanceSettings::cursor_theme` (`appearance::cursors::CursorTheme`),
  written back by `write_into` as the other theme axes are. Set it with
  `CursorTheme::load(&info.id)`.
- The list: `appearance::cursors::available()` -> `Vec<CursorThemeInfo>`, each
  with `id` (the folder name, an `OsString`: it need not be text), `name` (what
  the theme calls itself, else its folder name, escaped if it is not text) and
  `built_in`. The built-in theme -- the pointer as drawn today -- is always
  first; the rest are sorted by name. A theme installed in two places is listed
  once, as the copy that would be used.
- A preview, if you want one: `theme.cursor("default", 32)` gives the theme's
  arrow as premultiplied ARGB pixels (`frames[0]`); `"wait"` and `"progress"`
  are the animated ones. `None` means the theme does not draw it and the
  built-in pointer would be shown.

## What it asks

A chooser that lists `available()` by `name`, shows the current
`cursor_theme` as chosen (compare by `id()`), and saves the choice. The
existing `ReloadAppearance` path tells the compositor.
