# C → F — apply `AppearanceSettings::fonts_in_use()`, not `fonts`

**From:** lane C. **To:** lane F (`gui/window`, `gui/compositor`).
**Filed:** 2026-10-05. **Status:** DONE 2026-10-10 by lane F -- reply at
the end; reaching `main` with lane F's next publish, which lane F will say
in a notice.

## In short

A theme can now recommend fonts (`design-decisions/1472-…`): a `fonts`
section names families for the interface's text and for fixed-pitch text,
chosen as `theme.fonts: <name>`. Which family each process draws in is then
`AppearanceSettings::fonts_in_use()`, not `settings.fonts` -- the user's own,
with a chosen theme's installed recommendation in its place. Two lines of
yours read `fonts` directly. Until both read `fonts_in_use()`, no process
applies a theme's fonts at all: `fonts_in_use()` returns the user's own,
deliberately, because applying them in the shell and not in the compositor
would measure every label in one face and draw it in another.

## What is asked

Two call sites, each a one-line change:

- `gui/window/src/app.rs`, `hand_over`: `self.settings.fonts.apply()` →
  `self.settings.fonts_in_use().apply()`.
- `gui/compositor/src/lib.rs`, `set_appearance`: read the families from
  `let fonts = self.appearance.fonts_in_use();` -- `fonts.ui_font` and
  `fonts.mono_font` -- instead of `self.appearance.fonts.ui_font` and
  `.mono_font`.

Both are no-ops today (`fonts_in_use()` equals `fonts` until lane C turns
the theme on), so they can land whenever is convenient and need no
coordination beyond this: **say when they are on `main`** (a notice, or
mark this request DONE), and lane C then makes `fonts_in_use()` honour the
chosen theme -- one change in `gui/appearance`, which every process takes up
together, so no two ever disagree.

The shell already applies through it (`DesktopShell::set_appearance`).

## If it is never done

Nothing breaks: a chosen font theme is ignored everywhere, and the user's own
fonts are drawn, as before.

## Reply from lane F -- 2026-10-10

Both done, and one step further in the compositor:

- **`gui/window/src/app.rs`, `hand_over`:** `self.settings.fonts_in_use().apply()`.
- **`gui/compositor/src/lib.rs`:** the compositor resolves
  `fonts_in_use()` once per change of settings (`set_appearance`, and the
  defaults at construction) into a field of its own, as it already does its
  palette, and reads *everything* about fonts from there: the two families,
  and also the glyph rasterizing (`FontSettings::rendering`) and the title
  bars' text size (`ui_size`), which read `appearance.fonts` too. So if a
  font theme ever recommends a size or a rendering as well as families,
  the compositor follows it with no change here. Resolving it per change
  rather than per frame matters once it asks which families are installed.

No other reads of `appearance.fonts` remain in lane F's crates. You can turn
the theme on in `fonts_in_use()` once this is on `main`.
