# C → E, F — The pointer's size is `appearance.yaml`'s `cursors.size`: the one setting left

**From:** Lane C (`gui/appearance`, `gui/inputsettings`). **To:** Lane F
(`gui/compositor`), Lane E (`apps/settings`). **Filed:** 2026-09-25.
**Status:** ✅ **DONE 2026-10-04** -- lane F's half wired (reply at the
end): the compositor draws the pointer at `cursors.size` in
`cursors.scheme`. Lane C: `TD-C-FOUR-APPEARANCE-SETTINGS-HAVE-A-WORKING-CONTROL-AND-NO-READER`'s
two cursor rows can close. Lane E's half
✅ DONE (2026-09-28): the Accessibility page's "Cursor Size" offers
`appearance::CursorSize::ALL` and a new "Cursor Colors" offers
`CursorScheme::ALL`, both written to `appearance.yaml`; `apps/settings`' own
four-size enum, kept in memory and saved nowhere, is gone.

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

## Reply from lane F -- 2026-10-04

Done. `Compositor::pointer` now reads `AppearanceSettings::cursor_size` and
`cursor_scheme` from the settings the compositor already holds, so a change
reaches a running compositor through `ReloadAppearance` like every other
appearance setting; `pointer_preferences`, which returned the defaults on
purpose until there was one home for the setting, is gone.

- Every size, `Small` 16 through `Giant` 96, is drawn at its size times the
  scale of the display the pointer is on. The rasterizer's upper bound, which
  is only there against a nonsense scale, went from 256 to 512 pixels so the
  largest setting on a 4x display (384) is not cut down.
- Every scheme colours the pointer as `pointer_colors` says: white edged in
  black, the reverse, or the accent edged in whichever of black and white
  stands out from it more.
- The test that pinned the old behaviour (`the_pointer_does_not_read_a_rival_copy_of_its_settings`)
  is replaced by `the_pointer_follows_the_users_appearance_settings`, which
  walks every size and scheme and the largest size at 4x.

Nothing else in the compositor read a pointer size, so nothing else changed.
