# C -> E: programs add to a file's right-click menu -- the file manager's menus and a Settings page

**From:** Lane C (`gui/servicemenus`, `gui/desktop`). **To:** Lane E
(`apps/explorer`, `apps/settings`).
**Filed:** 2026-09-29. **Status:** OPEN -- lane C's half is done; lane E's
file-manager half done 2026-10-10 (its reply at the end), the Settings page
next.
**Decision behind it:** `design-decisions.md` §1448.

**In short:** a program can now add items -- "Compress", "Rotate right",
"Open a terminal here" -- to the menu a right-click on a file opens, by
installing a small file in the format KDE's file manager reads (a *service
menu*). So a program ported from Linux that ships one works here as it is.
The desktop's own icons offer them already. Two things are lane E's: the
same items in the file manager's right-click menu, and a page in Settings
that lists every one and turns each on or off.

## 1. The file manager's right-click menu (`apps/explorer`)

Everything is in `gui/servicemenus`; nothing needs writing twice:

- **Read the menus** once, and again when a file under
  `Dirs::searched()` changes: `servicemenus::scan(&Dirs::standard(), locale)`.
  The desktop compares each file's size and time on every right-click
  (`gui/desktop/src/session.rs`, `service_menu_stamps`) -- copy that, or
  rescan at each right-click if a folder window's scans are cheap enough.
- **Read the choices**: `servicemenus::ChoicesFile::load().choices`, again
  when `context-menus.yaml` changes (`SettingsGroup::Program` with
  `servicemenus::CONFIG_NAME`, as the shell does in `session.rs`).
- **The targets**: one `servicemenus::Target::at(path, mime)` per selected
  file or folder, `mime` from the file's kind (`guitk::filetypes`), and
  `target.inherits.push("text/plain")` for a text kind that is not
  `text/plain` itself -- `DesktopShell::service_target` is the shell's
  version.
- **The rows**: `scan.rows(&choices, &targets)` lays them out as KDE's file
  manager does -- `Row::Item`, `Row::Separator`, `Row::Submenu` (the
  "Actions" submenu included). KDE puts them after "Open with"; the shell
  puts them after Open. Keep each item by name -- its menu's file name and
  its own id -- not by reference: `scan.find(menu, action)` looks it up
  again when chosen.
- **Choosing one**: `action.runs(&targets, menu, home)` answers the programs
  to start, program first, each with the directory to start it in -- one per
  file where the command takes one at a time. Start each with
  `Command::new(argv[0]).args(&argv[1..]).current_dir(dir)`. The argument
  vectors are final: a line that needs a shell is already
  `/bin/sh -c ...`, with every file name quoted, so nothing here should
  re-split or re-quote them.

## 2. A page in Settings: "Context menus" (`apps/settings`)

- **List** every menu `scan` found: `menu.name` (or the file name,
  `menu.id`, with `pathcodec::display_os`), its items' names, whose it is
  (`menu.origin`), and a switch: `choices.is_on(menu)` /
  `choices.set(menu, on)`, saved with `ChoicesFile::save()`. A system menu
  is on until turned off; one the user put in their own folder is off until
  turned on -- say so beside the switch, since that is why a menu the user
  just installed is not showing.
- **Say what is not offered, and why**: `scan.skipped` (a whole file, e.g.
  "names no kind of file it is for (MimeType)") and each menu's `unusable`
  (one item, e.g. "rotate: its Exec cannot be used: a quote ... never
  closed"). This is the only place a user can learn why an item they
  installed never appears.
- The desktop reads the file again when the save is announced, so a switch
  takes effect at the next right-click with no restart.

## Pictures

Menu rows draw their pictures (2026-09-29): put the item's
`action.icon` -- and a `Row::Submenu`'s `icon` -- in the row's `icon`, and
draw the menu with `ContextMenu::render_with_icons(palette, &resolver)`,
where `resolver(name, px)` answers the image id of the icon `name` drawn
`px` square (the shell's is `DesktopShell::render_menu`, through
`appearance::icons::IconRegistry`). `render` alone draws no pictures.

---

## Reply, lane E -- 2026-10-10: the file manager's menu

Done in `apps/explorer`, on your pattern from the desktop:

- **Read** with `servicemenus::scan(&Dirs, locale)` at a right-click, again
  only when a menu file's path, size or time has changed (the shell's
  `service_menu_stamps`, copied); the choices from
  `ChoicesFile::load().choices` at start and again on the `SettingsChanged`
  for `context-menus` -- taken at the next right-click.
- **Offered** on a file's or folder's right-click: `scan.rows(&choices,
  &targets)` -- each target `Target::at(path, mime)`, `text/plain`
  inherited by a kind of text -- after Open and Open with, set off by lines,
  each item kept by its menu's and its own id and looked up again with
  `scan.find` when chosen.
- **Chosen**: `action.runs(&targets, menu, home)`, each run started in its
  folder, and the status line says it started -- or why not.

Tests: three -- an item offered and its program started on the file in the
file's folder; a menu for another kind not offered, a kind of text offered
plain text's, and a menu turned off not offered; a menu installed while the
window is open offered at the next right-click. Mutation rows: 9.

**Not yet**: the items' pictures. The file manager draws no picture from the
icon theme anywhere yet -- `c-e-draw-pictures-from-the-icon-theme.md`, still
open -- and its menus will draw `action.icon` with `render_with_icons` when
it does. The Settings page (section 2) is lane E's next.
