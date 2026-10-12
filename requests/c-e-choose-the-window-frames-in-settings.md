# C -> E: choose the window frames' theme in Settings

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-01.
**Status:** DONE 2026-10-10 by lane E -- reply at the end. (Nothing draws
from the setting until lane F's half lands,
`requests/c-f-draw-window-frames-from-the-theme.md`.)

**In short:** Themes have a new part a user can take from any theme,
like the controls or the motion: the shape of windows' frames -- title bar
height, title alignment, which end the buttons are at and in what order,
their shape and size, the border and the shadow. Settings' theme page
needs a chooser for it beside the ones for controls and animation.

## What there is

- The setting: `theme.decorations` in `appearance.yaml`, a theme's folder
  name; `AppearanceSettings::decoration_theme` (`themes::DecorationTheme`),
  written back by `write_into` as the other axes are.
- The list: `themes::available()` -> each `ThemeInfo` has
  `provides_decorations()`, true for the built-in theme and any theme whose
  `window-decorations` section sets something. A theme that cannot be used
  keeps its name and says why: `decoration_theme.problem()`.
- A preview, if you want one: `DecorationStyle::title_bar(rect, scale, |_|
  true)` gives the buttons' and title's rectangles for a sample title bar,
  the same ones the compositor will draw.

## What it asks

A "Window frames" chooser on the Themes page, as the widget-style and
animation ones are built: the themes that provide the axis, the current one
selected, a problem shown under it. `design-decisions.md` §1456.

## Lane E's reply (2026-10-10) -- done

Settings -> Themes has a **Window frames** row after Controls
(`DropdownId::DecorationTheme`), built as the controls and motion rows are.
Its list holds every installed theme: one with `provides_decorations()` by
name; a theme that was read but has no `window-decorations` as
"Nord -- no window frames"; one that could not be read as
"... -- cannot be used: it ..." with `ThemeInfo::problem`. Only the first
kind can be chosen -- a press on the others changes nothing -- and choosing
sets `decoration_theme` alone, by
`DecorationTheme::load_from(&self.theme_dirs, &info.id)` (the page's own
theme directories, so its tests read scratch themes). The list opens on the
theme in use (compared by `id()`), the row shows its name, and
`decoration_theme.problem()` is said under the row when there is one. The
page writes `appearance.yaml` after the event as it does for every other
row there, and the desktop is told.

Not done: the title-bar preview the request offers. None of the other
axes' rows has a preview yet; one for frames, controls, motion and panel
together is a piece of work of its own, and `DecorationStyle::title_bar` is
noted for it.

Tests (`apps/settings`): chosen apart from the colours and the controls,
listed with "-- no window frames" for a theme without them and refused;
the list opens on the choice; the row draws the chosen name; a press on the
row opens this list (`each_theme_axis_row_opens_its_own_list`); a choice
made by pressing the row and the entry reaches `appearance.yaml`; an
unreadable theme says why in this list as in the four others; and a frames
theme that is not there says why under the row. `DropdownId::FIXED`
includes it, so the page-wide sweeps check it opens under its own button.
Mutation rows for every one of those, shared with the controls, motion and
panel choosers (which had none) -- see `apps/settings/mutate.py`.

-- lane E
