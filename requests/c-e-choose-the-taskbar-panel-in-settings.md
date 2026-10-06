# C -> E: choose the taskbar panel's theme in Settings

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-01.
**Status:** OPEN -- the setting works without it (a hand edit of
`appearance.yaml`), and the desktop already draws from it.

**In short:** Themes have a new part a user can take from any theme: how
the taskbar is finished -- how much of the Aero glass it wears, from all of
it to a flat bar in the theme's colour -- and how far apart its buttons are.
Settings' theme page needs a chooser for it beside the ones for window
frames, controls and animation. `design-decisions.md` §1460.

## What there is

- The setting: `theme.taskbar_panel` in `appearance.yaml`, a theme's folder
  name; `AppearanceSettings::panel_theme` (`themes::PanelTheme`), written back
  by `write_into` as the other axes are. Set it with
  `themes::PanelTheme::load(&info.id)`.
- The list: `themes::available()` -> each `ThemeInfo` has `provides_panel()`,
  true for the built-in theme and any theme whose `taskbar-panel` section sets
  something it could read.
- Why a chosen theme is not in use, if it is not: `PanelTheme::problem()`, a
  sentence (`"nord" sets no taskbar panel, so the built-in taskbar panel is
  used.`); what in its file was ignored: `warnings()`.
- A preview, if wanted: `panel_theme.style()` -> `appearance::panel::PanelStyle`
  (`gloss` in hundredths, and the three gaps in pixels).

## What it asks

A chooser listing the themes that `provides_panel()`, showing the current
`panel_theme` as chosen (compare by `id()`), saving the choice, and showing
`problem()` when there is one. The desktop re-reads `appearance.yaml` on
`ReloadAppearance` as it does for the other axes.

Whether the bar is see-through is not this chooser's: that stays the
taskbar style and transparency the page already has.
