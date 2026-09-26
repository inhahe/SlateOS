# C → D — Install each program's desktop entry where the desktop looks for it

**From:** Lane C (`gui/desktopentry`, `gui/desktop`). **To:** Lane D (the root
filesystem recipe, `scripts/create-ext4-rootfs.sh`). **Filed:** 2026-09-26.
**Status:** OPEN -- nothing to do until programs with windows ship in the image.

**In short:** the start menu now lists the programs installed on the machine,
found by their desktop entries -- a `.desktop` text file per program -- in
`/usr/share/applications`, the directory every Linux desktop reads. Lane E is
asked to write one per program (`requests/c-e-ship-a-desktop-entry-with-each-program.md`).
When the image carries a program with a window, it should carry that program's
entry too, or the program is installed and missing from the menu.

## What to install

- **`/usr/share/applications/<id>.desktop`**, one per program with a window,
  from the program's crate (`apps/<name>/*.desktop`), unchanged. The directory
  itself can exist empty today; the desktop reads it at login and again when
  the start menu opens if it changed.
- **The entry's `Exec` must name the program where it is installed.** An entry
  saying `Exec=calculator` finds `/usr/bin/calculator` on `$PATH`; one naming
  an absolute path must name the one the image uses.
- Icons, if a program ships its own: `/usr/share/icons/hicolor/scalable/apps/<name>.svg`
  is where the Icon Theme Specification puts them. (The shell draws theme icons
  from its own icon themes today; reading `hicolor` is lane C's to add when a
  program ships one.)

## Where the desktop looks

The XDG Base Directory specification's data directories, first found wins:
`$XDG_DATA_HOME/applications` (`~/.local/share/applications`, the user's own),
then each of `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`) with
`/applications`. Nothing needs setting if the image uses `/usr/share`.

## What happens until it is done

Nothing breaks: today the image ships no programs with windows, and the menu
shows the shell's own list, as before.
