# C -> E: programs add to a file's right-click menu -- the file manager's menus and a Settings page

**From:** Lane C (`gui/servicemenus`, `gui/desktop`). **To:** Lane E
(`apps/explorer`, `apps/settings`).
**Filed:** 2026-09-29. **Status:** OPEN -- lane C's half is done.
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

## What lane C still owes

- Menu rows draw no icons (`known-issues.md`,
  `TD-C-MENU-ROWS-DRAW-NO-ICONS`): the names are kept for when they do.
