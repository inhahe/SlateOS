# C -> E: choose the window frames' theme in Settings

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-01.
**Status:** OPEN -- the setting works without it (a hand edit of
`appearance.yaml`), and nothing draws from it until lane F's half lands
(`requests/c-f-draw-window-frames-from-the-theme.md`).

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
