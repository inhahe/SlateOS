# C → E — fonts from a theme, on the Fonts page

**From:** lane C. **To:** lane E (`apps/settings`).
**Filed:** 2026-10-05. **Status:** OPEN.

## In short

A theme can now recommend the typefaces it was designed with
(`design-decisions/1472-…`): a `fonts` section lists families for the
desktop's text and for code, tried in order. It is chosen as the other parts
of a theme are, by `theme.fonts: <name>` in `appearance.yaml` -- and nothing
in Settings can set that yet, nor offer to install a recommended family this
machine lacks, which is the roadmap's half of the feature (`roadmap-detailed.md`,
*Tier 2 -- Font Preferences*: "Settings app offers to install recommended
fonts from package manager").

## What is asked

On the Fonts page:

- **"From a theme"** beside choosing a family of one's own: the installed
  themes for which `appearance::themes::ThemeInfo::provides_fonts()` is true,
  each with its recommendations (`ThemeInfo::fonts`: `ui` and `mono`, in the
  order they are tried). Choosing one sets
  `settings.font_theme = FontTheme::load(id)`; choosing a family of one's own
  sets it back to `FontTheme::built_in()` -- otherwise the family chosen is
  not the one drawn.
- **Which recommendations are installed**:
  `guitk::text::family_installed(name)`. The family that will be drawn is
  `AppearanceSettings::fonts_in_use()` (`ui_font`, `mono_font`).
- **Installing the rest** through the package manager, where it has the
  family -- the roadmap's "offers to install".
- **When a chosen theme cannot be used** -- not installed, or recommending no
  font -- `FontTheme::problem()` says why in a sentence; the user's own fonts
  are drawn meanwhile.

Note that `fonts_in_use()` returns the user's own fonts until lane F's two
call sites apply through it (`requests/c-f-apply-the-fonts-in-use.md`); the
page can be built against it now and will show the theme's families from
the day that lands.

## If it is never done

Nothing breaks: the user's own fonts are drawn, as before, unless
`theme.fonts` is written by hand.
