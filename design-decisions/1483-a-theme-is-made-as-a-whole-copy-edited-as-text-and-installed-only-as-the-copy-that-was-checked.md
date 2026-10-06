## 1483. A theme is made as a whole copy, edited as text, and installed only as the copy that was checked -- and the chosen themes' folders are followed with the settings

**Date:** 2026-10-06 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** a user can now make a theme of their own from any theme they
have -- the built-in one included -- change its colours and what describes
it, install a theme someone sent them, copy a theme out to share, and remove
one of their own. Each is one function in `appearance::themes::authoring`,
which the Settings app's theme editor is to use, and the new `theme` program
drives from a terminal (`theme derive nord "Nord, warmer"`, `theme set ...
colors base 3b4252`). And when the theme in use is edited -- by either of
those, or by hand in a text editor -- or the picture on the desktop is saved
over under its own name, every window shows the change at once, where before
nothing noticed until another setting changed.

**Where:** `gui/appearance/src/themes/authoring.rs` (the model),
`gui/appearance/src/bin/theme.rs` (the program),
`gui/appearance/src/themes.rs` (`dependency_folders`, `chosen`),
`gui/appearance/src/lib.rs` (`dependency_paths`),
`gui/settingswatch/src/lib.rs` (`Dependents`, `Policy::dependent_event`,
`Watcher::following`), `gui/desktop/src/session.rs` (`watch_settings`).
Asked for by `roadmap-detailed.md` §4.6 (*Theme Editor in Settings App*:
"Derive from existing", "Import/Export") and `design.txt` ("individual desktop
colors? (make your own theme)").

### The choices, and what they cost

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Only the user's own themes change**; a system theme is changed by deriving a copy -- under its own name, to stand in for it, as `themes` already lets a user's copy do | editing a system theme in place | A theme installed with the system belongs to every user and to the next update; a copy is the user's, and removing it brings the system's back. | Changing one colour of a system theme copies the whole folder. |
| **A derived theme is a whole copy of the folder** -- icons, cursors, pictures and all | a file that refers to its source for what it does not change | It keeps working when its source is updated or removed, and it can be shared as it stands. The format has no way for one theme to refer to another, and adding one would make a theme's meaning depend on what else is installed. | A copy of a theme with large pictures takes their room again. |
| **The built-in theme is derived from its template and its icons, compiled in** | refusing, or copying the system's installed folder | It can be derived from on a machine with nothing installed. The template is held to the compiled palette by tests already. Each icon is a file of its own, so a name sharing another's picture is drawn exactly as the built-in theme draws it. | About 70 small SVG files in a theme that may only want new colours. |
| **A copy's screenshots are left out; its author and licence stay** | keeping everything, or making the user the author | Screenshots picture the source -- a theme browser would show them as pictures of the copy, and the repository's review asks whether screenshots match. A licence that asks for credit is honoured by leaving the credit where it is; the user changes `author` when they share. | A copy shared without new screenshots has none until its author takes some. |
| **A theme being changed is its file as text, edited through `yamldoc`; its meaning is always `themes::parse` of that text** | a typed model written back out | A colour set is one line changed: the author's comments, order and spelling survive. A preview drawn from the draft is what the desktop will draw, since the same reader reads both. | Only the colour sections and `meta` have typed setters; the other axes are carried as written. |
| **Install copies first and checks the copy** (`themecheck`), and only a copy that passes is put in place | checking the source, then copying | What is checked is what is installed: nothing can change between the two, and what the copy left out is part of the verdict. | A theme that fails is copied in full before it is refused (then deleted). |
| **A copy follows the checker's reading of a folder**: links to a file inside become hard links; hidden entries, links leading out or to folders, devices and oversized files are left out and said | copying the tree as it is | A copy reads nothing outside the folder, so a shared theme cannot carry one of the user's files out with it. Hard links keep a cursor theme's dozens of alias names from costing a copy each. | A theme that worked through a link to a shared folder loses that file in a copy, and says so. |
| **Every folder is built inside a hidden holder beside where it goes and renamed into place; a removed theme is renamed aside before it is deleted** | writing in place | A crash leaves the old theme or the new one, never half a theme in the list. | A crash can leave a hidden folder no list shows. |
| **The desktop's settings watcher follows what the appearance settings name -- the chosen themes' folders, the themes directories, and the user's own pictures shown (wallpaper, a schedule's, the login screen's) -- and announces a change there as `appearance`** (`settingswatch::Dependents`, `appearance::dependency_paths`) | every program that saves a theme or a picture also announcing it | A theme edited by anything -- the editor, the `theme` program, a text editor -- and a picture saved over by any program reach every window, where a rule for savers would miss the text editor and the paint program. Each window's watcher already compares the theme's bytes, and the desktop its pictures' stamps, so a change that alters nothing they read costs a look and no more. This also closes `TD-C-A-WALLPAPER-REPLACED-UNDER-THE-SAME-NAME-IS-NOT-READ-AGAIN`'s last part. | An unrelated theme installed beside the chosen ones also costs every window a look. |
| **A folder is followed whole; anything else by its name in the folder holding it** | following every path's folder whole | A wallpaper sits in a pictures folder that may hold thousands: a neighbour saved there costs nothing. A path not there yet can still be followed, by its name, while its folder is. | A program that saves by writing a new file under another name and renaming it over is seen at the rename -- as `settingsfile` itself saves, and as most editors do. |
| **When the paths to follow change, the watch is made afresh -- the old one read out first** | removing and adding single watches | `libcall`'s inotify has no `rm_watch`, and a whole new instance is simpler to get right; reading the old one out after the new one is watching loses nothing in between. | A watch is rebuilt each time a theme or a picture is chosen, installed or removed -- rare events. |
| **A themes directory not made yet is followed as the first folder missing on the way to it, by its name in the folder above** (up to two missing) | following the nearest folder there whole, or nothing | The first theme a user installs, which makes the directory, is seen -- and nothing else in `~/.local/share`, which every program writes to, is. | A directory with more than two missing folders above it is not followed until something makes them. |

### What it does not do

- **No editor yet.** The Settings app's page is lane E's
  (`requests/c-e-a-theme-editor-on-the-themes-page.md`).
- **Only colours and `meta` have setters.** The other axes -- widget style,
  animation, frames, panel, fonts, wallpapers -- are carried as the source
  wrote them; a typed setter for each is the editor's next need.
- **No theme repository.** Importing "from the theme repository" waits on the
  repository (§4.6's distribution items); a downloaded theme's folder or file
  installs through `install` already.
- **Icons, cursors and sounds edited in place are not followed**: the
  settings' fingerprint does not read them either (they are read when drawn
  or played), so following them would announce changes no watcher compares.
