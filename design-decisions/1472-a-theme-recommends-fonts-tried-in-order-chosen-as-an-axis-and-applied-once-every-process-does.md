## 1472. A theme recommends fonts, tried in order, chosen as an axis -- and applied only once every process applies them

**Date:** 2026-10-05 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a theme can now name the typefaces it was designed with -- one
list for the desktop's text and one for code -- without shipping them, since
a font's licence seldom lets a theme carry it. Choosing that theme for the
fonts (`theme.fonts: <name>` in `appearance.yaml`, as every other part of a
theme is chosen) draws in the first family of each list the machine has, and
in the user's own font where it has none of them. Sizes stay the user's. It
is not switched on yet: every program that draws text has to ask for the
fonts in the same way first, or the desktop would measure text in one face
while the compositor draws it in another. Two of those calls are lane F's;
when they are in, one change here turns it on for all of them at once.

**Where:** `gui/appearance/src/themes/fonts.rs` (`FontNames`, `FontTheme`,
`families_in_use`), `gui/appearance/src/themes.rs` (`ThemeFile::fonts`,
`ThemeInfo::fonts`, `provides_fonts`), `gui/appearance/src/lib.rs`
(`AppearanceSettings::font_theme`, `fonts_in_use`),
`gui/appearance/src/themecheck.rs` (the `fonts` axis),
`gui/toolkit/src/text.rs` (`family_installed`); asked for by
`roadmap-detailed.md` (*Tier 2 -- Font Preferences*).

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Recommend, never bundle** | a `fonts/` folder in the theme | What the roadmap asks, and why: a theme is shared, and most typefaces' licences do not let it be shared with them. Settings offers to install what is recommended (lane E). | A theme looks as designed only where its fonts are installed. |
| **A list for each role, the first installed used** | one name | CSS's `font-family` answer: a theme names its first choice and the fallbacks it was designed with, so it degrades the way its author chose rather than straight to the user's font. | A list is read further than one name -- a few lookups in an index already in memory. |
| **The user's own font where none is installed** | the toolkit's default list | The user chose that one; the default list is for a user who chose nothing. | -- |
| **Families only: no sizes, no weight** | the whole `fonts` section of the settings | How large text is drawn is the reader's, as the scale is; a theme that made text smaller would make it unreadable to someone, and themes do not control layout (`roadmap-detailed.md`). | A theme whose face reads small at the user's size cannot correct for it. |
| **An axis, `theme.fonts`** | a switch, "use the colour theme's fonts" | The model of every other axis: one theme's colours with another's fonts. | One more key. |
| **Off until every process applies the same way** | on now in the processes lane C owns | A label is measured by the program that lays it out and drawn by the compositor. If the shell drew in the theme's face and the compositor in the user's, every centred label would sit off centre and every cut one be cut in the wrong place -- the failure `guitk::text::install_ui_faces` is documented against. `fonts_in_use()` is the one question every process asks; until the compositor and the applications ask it (`requests/c-f-apply-the-fonts-in-use.md`) it answers with the user's own fonts, and turning the theme on is then one change that every process takes up together. | Until lane F's two lines land, a chosen font theme is read, listed and checked, and drawn nowhere. |

`themecheck` checks the section: a name that is no family's is an error, a
recommended family this machine lacks is a note (a theme is checked where it
is written, not where it is used), and a fixed-pitch recommendation that is
installed here and not fixed-pitch is a warning -- a terminal drawn in a
proportional face loses its grid.

**What is not done.** The Settings page -- the themes that recommend fonts,
which of their families are installed, and installing the rest -- is lane
E's (`requests/c-e-fonts-from-a-theme-on-the-fonts-page.md`).
