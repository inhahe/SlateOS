## 884. The start menu is a tree of the installed programs, read from their desktop entries

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The start menu listed ten programs typed into the shell's own
source. It now lists the programs installed on the machine, as each one's
*desktop entry* -- a small standard text file saying the program's name,
picture, kind and command -- describes it, grouped into folders by kind
(Accessories, Graphics, Internet...): the "applications tree" `design.txt`
asks for. The folders start open and close with a click. No program ships an
entry yet, so the menu still shows the shell's own ten, now in folders.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| How a program says what it is | the freedesktop Desktop Entry Specification's `.desktop` files, read by a crate of our own (`gui/desktopentry`) | a SlateOS manifest format | every Linux desktop reads these, so a ported program brings its entry and one written here is read elsewhere. |
| Where they are looked for | the XDG data directories, the user's first; the first file per desktop file ID wins | `/usr/share/applications` only | a user's own copy then overrides a system entry, and a copy saying `Hidden=true` removes it -- the specification's way, with no second mechanism. |
| When they are read | at login, and when the start menu opens if any entry file was added, removed or rewritten (size and time per file) | a directory watch; or the directories' own times | there is no watch here yet; a directory's time moves on some filesystems when a file is added and on none when one is edited in place. A stat per entry is cheap beside the read and parse it saves. |
| Which folder a program is in | the first *main* category its entry names | every main category it names | `Audio;Video;AudioVideo` would list a player three times. The author's first answer is the one that counts. |
| Folders open or closed at first | open, closed by a click for the session | closed | closed, everything is two clicks away, and with a handful of programs the menu is a list of folders. Open, it reads as a grouped list, and a folder the user never uses can be closed. |
| The shell's own list | kept, each program replaced by an installed entry for the same file name | dropped when entries exist | until programs ship entries it is the only menu there is; once one does, its entry wins, with its own name and picture. |
| Starting a program | its entry's `Exec` expanded into an argument vector; `Terminal=true` as `terminal -e ...` | the program with no arguments | the entry's arguments are part of how the program is meant to start. Never a shell string: a file name stays one argument, as bytes. |

### What is not done here

- **Programs ship no entries yet** (`requests/c-e-ship-a-desktop-entry-with-each-program.md`),
  and the image installs none (`requests/c-d-install-desktop-entries-into-the-image.md`).
  The terminal ignores `-e`, so a `Terminal=true` program opens a shell (lane E).
- ~~Icons come from the icon theme only.~~ Done the same day: an entry naming
  an icon file by path, or an icon installed in `hicolor` or `pixmaps`, is
  drawn -- SVG, or PNG scaled by area -- after the chosen theme and, name by
  name, beside the built-in set (`appearance::icons::IconTheme::render`).
- ~~Jump lists (an entry's `Actions`) are read and not yet offered.~~ Offered
  the same day: a program's right-click menu -- its start menu row, its pinned
  taskbar button -- starts with its actions, above a separator; an action with
  no command line (D-Bus only) is left out.
- **Which folders are closed** is not saved across logins.
