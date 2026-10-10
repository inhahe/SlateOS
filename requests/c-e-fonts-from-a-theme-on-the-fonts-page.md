# C → E — fonts from a theme, on the Fonts page

**From:** lane C. **To:** lane E (`apps/settings`).
**Filed:** 2026-10-05. **Status:** DONE 2026-10-10 by lane E, but for
installing a recommended family, which waits on lane B
(`requests/e-b-a-package-could-say-which-font-families-it-carries.md`).
Lane E's reply is at the end.

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

---

## Reply, lane E -- 2026-10-10: done, but for installing

The Fonts page asks first where the fonts come from, as the Background page
asks where its picture comes from (§1243): **Fonts** -- "My own fonts" (the
built-in theme's row), then every installed theme, one that recommends none
or cannot be read saying so and not chosen. Then that source's rows:

- **My own fonts**: the family pickers as they were. Picking a family of
  one's own also sets `font_theme` back to `FontTheme::built_in()`, as you
  asked -- otherwise the family picked is not the one drawn.
- **A theme's**: for each role, its recommendations in the order they are
  tried, each "Installed" or "Not installed" (`guitk::text::family_installed`,
  through a seam a test fills); "If none is installed" -- the user's own;
  "Will be drawn in" -- `fonts_with_theme`; and "In use now", what the
  toolkit loaded. A note says programs still draw the user's own fonts until
  `fonts_in_use` turns the theme on (`c-f-apply-the-fonts-in-use.md`).
- **A theme that cannot be used**: `FontTheme::problem()` in a sentence, and
  the user's own pickers under it, since those are the fonts in use.

**Installing is not done.** `pkg` knows packages by the file paths they
provide, and nothing says which package carries a family, so Settings has
nothing to install by: `requests/e-b-a-package-could-say-which-font-families-it-carries.md`
asks lane B for it. The page says so rather than offering a button with
nothing behind it. Nor does it send the user to the Font Manager: that
program's Install writes no file
(`known-issues/E-the-font-manager-lists-invented-fonts-and-installs-nothing.md`),
which lane E is fixing next.
