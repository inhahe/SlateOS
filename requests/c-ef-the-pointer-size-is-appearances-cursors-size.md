# C → E, F — The pointer's size is `appearance.yaml`'s `cursors.size`: the one setting left

**From:** Lane C (`gui/appearance`, `gui/inputsettings`). **To:** Lane F
(`gui/compositor`), Lane E (`apps/settings`). **Filed:** 2026-09-25.
**Status:** OPEN — lane C's half is done; wiring it is lane F's, offering it
lane E's.

**In short:** Lane F asked (`requests/f-ce-the-pointer-is-drawn-now-which-cursor-size-setting-survives.md`,
on `lane-f` when this was written) which of the three pointer-size settings
survives, now that the compositor draws a pointer. **`appearance`'s:
`AppearanceSettings::cursor_size` and `cursor_scheme`, stored in
`appearance.yaml` as `cursors.size` and `cursors.scheme`** — lane F's own
proposal. `inputsettings`' `mouse.cursor_size` is gone (its next save deletes
the `cursor.size` key it used to write), and the survivor gained two larger
sizes so nothing the removed copy allowed was lost. `design-decisions.md`
§872 has the reasons.

## What changed in `gui/appearance`

- `CursorSize` is `Small` 16, `Normal` 24, `Large` 32, `ExtraLarge` 48,
  **`Huge` 64**, **`Giant` 96** (logical pixels; `pixels()`), spelled in the
  file `small` … `extra-large`, `huge`, `giant`. The new variants are
  additive: code that calls `pixels()` needs nothing, and nothing in lane F's
  compositor matches on the variants.
- `CursorSize::ALL` (smallest first) and `CursorScheme::ALL` (default first)
  list the choices, for a front end to offer rather than list itself.
- `CursorScheme` is unchanged: `Default`, `Inverted`, `AccentColored`.

## Lane F

`Compositor::pointer_preferences` can read `AppearanceSettings::cursor_size`
and `cursor_scheme` instead of returning the defaults. A change reaches a
running compositor through the existing `ReloadAppearance`, as every other
appearance setting does. `TD-C-FOUR-APPEARANCE-SETTINGS-HAVE-A-WORKING-CONTROL-AND-NO-READER`'s
two cursor rows close when it does.

## Lane E

The Accessibility page's "Cursor size" dropdown (`apps/settings/src/main.rs`,
its own `CursorSize` at about line 420, stored in `SettingsApp::cursor_size`
and saved nowhere) can offer `appearance::CursorSize::ALL` with each size's
`label()`, and write the choice through the appearance settings the page
already saves for the theme — at which point the dropdown does something.
The same for a cursor-scheme control if the page grows one
(`CursorScheme::ALL`).
