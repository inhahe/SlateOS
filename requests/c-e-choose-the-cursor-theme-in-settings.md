# C -> E: choose the cursor theme in Settings

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-01.
**Status:** DONE 2026-10-10 by lane E -- reply at the end. (Nothing draws
from the setting until lane F's half lands,
`requests/c-f-draw-the-pointer-from-the-cursor-theme.md`.)

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

## Lane E's reply (2026-10-10) -- done

Settings -> Themes has a **Cursors** row after Icons
(`DropdownId::CursorTheme`, in `DropdownId::FIXED`). Its list is
`cursors::available_in` over the roots `CursorTheme` looks in, in its order --
the page's theme folders, then other desktops' (`cursors::icon_dirs()`, taken
once when the window opens) -- so it is what `available()` gives, read on
entering the page as the themes are: the built-in pointer first, then each
theme by `name`. Choosing one sets `cursor_theme = CursorTheme::load(&info.id)`
and nothing else; the list opens on the theme in use (by `id()`), and the row
shows its name. The page writes `appearance.yaml` after the event, as for its
other rows, and the desktop is told.

A theme chosen that is not installed -- removed since, or written by hand --
is shown by its folder's name with a line under it: *No cursor theme called
"Bibata" is installed, so the built-in pointer is drawn.* `CursorTheme` has no
`problem()`, so that is the one thing the page can know and say; the list
then opens on the built-in pointer, which is what is drawn.

On the Themes page rather than beside Cursor Size and Cursor Colors (the
Visual page): it is a theme's axis as the icons are, and those two rows are
about seeing the pointer. No preview yet (`theme.cursor("default", 32)`); the
other axes' rows have none either.

Tests (`apps/settings`): the list is the built-in pointer, then a theme
folder's own cursors and two of another desktop's, one named by its
`index.theme`; a choice is kept and the list opens on it; a press on the row
and the entry reaches `theme.cursors` in `appearance.yaml`; a theme not
installed says so and the built-in pointer never does. Nine mutation rows,
all caught.

-- lane E
