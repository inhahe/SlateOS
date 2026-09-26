# C → E — A colour-theme picker for the Settings app's Appearance page

**From:** Lane C (`gui/appearance`). **To:** Lane E (`apps/settings`).
**Filed:** 2026-09-25. **Status:** OPEN -- lane C's half is done.

**In short:** the desktop can now be drawn in a colour theme other than its own
(`design-decisions.md` §874). A theme is a folder holding `theme.yaml` that sets
some or all of the palette's colours, and `theme.colors` in `appearance.yaml`
chooses one. Every program already honours the choice. What is missing is a way
to make it without editing that file: a list of the installed themes on the
Appearance page, near the "Theme Mode" cards (`apps/settings/src/main.rs`,
about line 3866).

## The API (`gui/appearance/src/themes.rs`)

- `appearance::themes::available() -> Vec<ThemeInfo>` -- every installed
  theme, the built-in one first and the rest by name. Reads the theme folders,
  so call it when the page opens, not per frame.
- `ThemeInfo`: `id` (what to store), `name` (what to show), `meta` (author,
  version, licence, tags), `screenshots` (paths, already confined to the
  theme's folder), `has_dark` / `has_light`, `warnings` (what in the file was
  ignored), `problem` (why it cannot be read, if it cannot).
  `provides_colors()` says whether it can be chosen for colours; list the others
  greyed with `problem` as the reason, so a theme's author can see why theirs
  is not offered.
- To choose one: `settings.color_theme = appearance::themes::ColorTheme::load(&info.id)`,
  then save as for any other setting. `load` reads the file, so the palette the
  page previews with -- `Palette::from_settings(&settings)` -- is the theme's at
  once, before saving.
- `settings.color_theme.problem()` is a sentence saying why the chosen theme is
  not in use (uninstalled since it was chosen, say). Worth showing under the
  list: the shell also posts it as a notice, but the page is where the user
  goes to fix it.

## Things a picker will want to say

- **"Dark only" / "Light only"** when a theme sets one mode: such a theme is
  shown in that mode whichever mode is chosen (§874), so the Theme Mode cards
  will appear to do nothing while it is selected. Saying so beside the theme
  is kinder than letting the user discover it.
- **The accent still applies.** A theme never sets the accent; the accent
  picker below keeps working under any theme.
- **High contrast wins.** With a high-contrast scheme on, no theme's colours
  show. Worth a line when both are set.

## Icons too (added 2026-09-26)

A theme can now carry icons, and the icons are chosen apart from the colours
(`design-decisions.md` §880): `theme.icons` in `appearance.yaml`.

- `ThemeInfo::has_icons` says whether a listed theme draws icons. A folder of
  icons with no `theme.yaml` -- an icon pack -- is listed with `has_icons` and
  no colours, and nothing wrong with it (`problem` is `None`).
- To choose one: `settings.icon_theme = appearance::icons::IconTheme::load(&info.id)`,
  then save. Nothing is read until an icon is drawn.
- A preview: `IconTheme::render(name, size, color)` gives an icon's pixels
  (straight-alpha `0xAARRGGBB`) for, say, `folder`, `user-home` and
  `utilities-terminal` beside each listed theme.

## What happens until it is done

Nothing breaks. A theme can be chosen by adding `colors: <name>` under `theme:`
in `~/.config/slateos/appearance.yaml`. The Settings app's own saves keep that
key -- `AppearanceSettings::write_into` writes it -- so a hand-chosen theme
survives the Settings app.
