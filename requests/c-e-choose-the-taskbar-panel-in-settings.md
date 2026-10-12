# C -> E: choose the taskbar panel's theme in Settings

**From:** Lane C. **To:** Lane E. **Filed:** 2026-10-01.
**Status:** DONE 2026-10-10 by lane E -- reply at the end.

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

## Lane E's reply (2026-10-10) -- done

Settings -> Themes has a **Taskbar panel** row after Window frames
(`DropdownId::PanelTheme`), built as the controls, motion and frames rows
are. Its list holds every installed theme: one with `provides_panel()` by
name; one read but with no `taskbar-panel` as "Nord -- no taskbar panel";
one that could not be read as "... -- cannot be used: it ...". Only the
first kind can be chosen, and choosing sets `panel_theme` alone, by
`PanelTheme::load_from(&self.theme_dirs, &info.id)` -- `load_from` rather
than `load` so the page reads its own theme directories, which its tests
point at scratch themes. The list opens on the theme in use (compared by
`id()`), the row shows its name, and `problem()`'s sentence is said under
it when there is one. The page writes `appearance.yaml` after the event
and the desktop is told, as for the page's other rows.

Not shown: `warnings()`. No axis's row says its theme's warnings yet (the
colours' included); if they should, that is one change for all of them.
No preview either, for the same reason as the frames' (see
`c-e-choose-the-window-frames-in-settings.md`).

Tests (`apps/settings`): chosen apart from the colours, the controls and
the frames; "-- no taskbar panel" listed and refused; the list opens on the
choice; the row draws the chosen name; a press on the row opens this list;
a choice made by pressing the row and the entry reaches `appearance.yaml`
(`saved.panel_theme.id() == "flatbar"`); an unreadable theme says why; a
theme with no panel chosen by hand says why under the row. In
`DropdownId::FIXED`, and mutation rows for each.

-- lane E
