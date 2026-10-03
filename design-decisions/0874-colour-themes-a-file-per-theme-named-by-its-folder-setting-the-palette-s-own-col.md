## 874. Colour themes: a file per theme, named by its folder, setting the palette's own colour names -- never the accent, never unreadable text

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** A user can now choose a colour theme: a small YAML file in a
folder named for the theme, setting some or all of the desktop's colours for
dark mode, light mode or both. What it leaves out stays as built in. The choice
is one line of `appearance.yaml` -- `theme.colors: nord` -- and every program
picks the colours up the next time it reads the settings. Getting there took
several smaller calls, each easy to reverse and listed below so any of them
can be overruled.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| What does a theme call its colours? | the palette's own role names -- `base`, `surface0`, `text`, `red` ... (`guitk::palette::THEME_ROLES`) | the roadmap's example names -- `background`, `surface`, `error`, `text-dim` | applications already read their colours by the palette's names, so a theme redefines exactly what they draw with; a second vocabulary is a translation table that has to be kept in step. The roadmap's list ends in "etc.": an example, not a spec. The cost is that Catppuccin's names (`crust`, `mantle`) explain themselves less, so the shipped template comments each group. |
| Can a theme set the accent? | no -- reported, and ignored | yes | the accent is the user's choice in Settings (§816 keeps it configurable even under high contrast); a theme that set it would quietly undo that choice. |
| Can a theme make text unreadable? | no -- the four text colours are held to the same contrast floor as the built-in ones, `text` included | trust the theme | a theme is data from strangers, and unreadable text is the one failure a user cannot get out of: the Settings page that would undo it is drawn in it. The built-in `text` is still left exactly as stated -- it already passes. |
| A theme with only dark colours, in light mode? | shown in its dark colours, and the palette says it is dark | the built-in light colours | the user has just chosen this theme; showing the built-in colours would make choosing it look broken. Laying its dark text over the light pages would be neither. |
| Where are themes installed? | `/usr/share/slateos/themes/<name>/theme.yaml`, and the user's own under `$XDG_DATA_HOME` (else `~/.local/share`) `/slateos/themes/`, the user's copy winning | `~/.config/slateos/themes` | a theme is a thing the user has, like a font -- fonts live in `~/.local/share/fonts` -- not a preference they set. The preference is `theme.colors`. |
| How is the choice written in `appearance.yaml`? | the folder's name, percent-encoded as filenames are (§426) | text only | a folder's name need not be text. The key is new, so there are no files from before the encoding and no version marker is needed. |
| When is the theme read? | when `appearance.yaml` is (`AppearanceSettings::read_from`), and never per frame | by each program that wants it | every reader of the settings -- the shell, the compositor, every application through `oswindow` -- gets the colours with them, with no second step to forget (§856: stored, and nothing obeyed it). |
| What is the built-in theme called? | `aero`, after the roadmap's "Default Theme -- Aero" and the demo §815 follows | `default`, `slate` | a theme needs a name it can be chosen back by. Its colours are compiled in, so it reads no file and cannot fail to load. |
| Is there a file for it? | yes -- `gui/appearance/themes/aero/theme.yaml`, every role, held equal to the compiled palette by a test | no | the roadmap asks for the default theme to be "a normal YAML theme file", and a theme's author needs a complete template to copy. |
| A theme that cannot be used? | the built-in colours, the choice kept, and one notice from the shell per loss | refuse the setting, or forget it | a theme uninstalled since it was chosen would otherwise look like a setting that quietly stopped working; keeping the name means saving some unrelated setting does not throw the choice away. |

### Changed along the way

- A high-contrast palette's text colours are now what its contrast floor
  starts from (`Palette::high_contrast`), so a style change keeps them instead
  of putting the ordinary mode's back -- the trap `Palette::from_settings`'s
  comment described, closed rather than documented.
- `settingsfile::testing::with_scratch_config` points `XDG_DATA_HOME` into the
  scratch directory as well, so a test that names a theme cannot find the
  developer's own.
- The desktop's three kinds of news about itself (a wallpaper not shown, a
  layout not saved, a theme not usable) are posted by one
  `ShellSession::post_desktop_notice` instead of three copies.

### Not done here

- A theme picker in Settings is lane E's (`requests/c-e-a-colour-theme-picker.md`);
  installing the template with the system is lane D's
  (`requests/c-d-install-the-built-in-colour-theme.md`).
- A theme file edited while it is the chosen theme is not noticed:
  `known-issues.md` `TD-C-AN-EDITED-THEME-FILE-IS-NOT-NOTICED-UNTIL-THE-SETTINGS-CHANGE`.
- The other axes -- window decorations, icons, cursors, sounds, terminal
  colours -- are not read yet. `meta.supports` is kept for a theme browser to
  show and is not trusted over what a file actually sets.
