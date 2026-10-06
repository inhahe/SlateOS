# C -> E: a theme editor on the Themes page -- the model is there

**From:** Lane C (`gui/appearance`). **To:** Lane E (`apps/settings`).
**Filed:** 2026-10-06. **Status:** OPEN -- nothing of lane E's breaks
without it; a user can make, change, install and share themes from a
terminal today (`theme`), and the page is how a user without one does.

**In short:** `roadmap-detailed.md` §4.6 asks for a theme editor in Settings
-- "Derive from existing", a colour picker for each colour with its contrast
shown, "Import/Export", a live preview. Everything under it now exists in
`appearance::themes::authoring` (design-decisions §1483): make a theme of
your own from any theme (the built-in one included), change its colours and
its name, author, version, licence and tags, install a theme from a folder
or a lone `theme.yaml`, copy a theme out to share, remove one of your own.
What is missing is the page. Every function says what went wrong in a
sentence (`AuthoringError`'s `Display`), so the page has no wording of its
own to invent for failures.

## What there is

| The page wants to | Call | Notes |
|---|---|---|
| list what can be edited | `themes::available_in(&dirs)` | `ThemeInfo::origin == Origin::User` is the user's own -- the only ones that open for editing; offer "Make a copy to change" on the others |
| start a new theme from one | `authoring::derive(&dirs, from_id, &authoring::id_for(&dirs, name), name)` | returns the new theme open, and what the copy left out (findings to show, usually none). From the built-in theme (`themes::BUILT_IN`) it is the template with every value written out |
| open one of the user's | `authoring::ThemeDraft::open(&dirs, id)` | `NotTheUsers` for a system or built-in theme |
| show a theme's colours | `draft.file()` -- `ColorSection::ALL`, `section.roles()`, `section.of(&file.colors).get(role)` | `file()` is the desktop's own reading of the text as it stands, so it is also what to preview |
| preview it | `ColorTheme::from_colors(draft.id(), draft.file().colors)` into the settings the preview draws from | the Themes page's `render_theme_preview` already draws through `Palette`; the draft is one more `ColorTheme` |
| set a colour | `draft.set_color(section, role, color)` | refuses the accent and anything see-through, with a sentence; contrast to show beside it: `guitk::theme::contrast_ratio` against `Palette::text_grounds` (roadmap §4.6's note) |
| put a colour back to the built-in one | `draft.clear_color(section, role)` | |
| name, author, version, licence | `draft.set_meta(MetaText::Name, "...")` (blank removes it) | |
| tags | `draft.set_tags(&["warm", "evening"])` | |
| warn of what the desktop will ignore | `draft.file().warnings` | the same sentences `themecheck` and the desktop give |
| know whether to offer Save | `draft.is_changed()` | |
| save | `draft.save()` | whole or not at all; nothing more is needed for the desktop to show it -- the desktop follows the chosen themes' folders now (§1483), so no `ReloadAppearance` either |
| cancel | open it again | the file on disk is untouched until `save` |
| import (a folder or a `theme.yaml`) | `authoring::install(&dirs, path, &authoring::id_for(&dirs, name))` | the copy is checked before it is put in place; `Refused(report)` carries every finding to show, `report.findings` |
| export | `authoring::export(&dirs, id, destination)` | the destination must not exist; the returned report is the check a theme repository would run -- worth showing before the user shares it |
| save the look the user has put together as one theme ("export current customizations") | `authoring::compose(&dirs, &settings, &authoring::id_for(&dirs, name), name)` | each axis as the theme chosen for it writes it; the wallpaper and icon themes' files copied in; what it says (cursors and sounds left to their own themes, a chosen theme not installed) is worth showing. Then `export` to share it |
| remove | `authoring::remove(&dirs, id)` | only the user's own; a choice of it in `appearance.yaml` stays and shows the built-in colours with a problem, as for any missing theme -- choose another first, or ask |

`dirs` is `themes::ThemeDirs::standard()` -- the page already holds one
(`theme_dirs`).

## Not yet in the model

- Setters for the other axes -- widget style, animation, window frames,
  taskbar panel, fonts, wallpapers. A copy carries them as its source wrote
  them. If the page wants to edit one, say which and lane C adds the setter.
- Importing "from the theme repository": there is no repository yet; a
  downloaded theme installs through `install`.
