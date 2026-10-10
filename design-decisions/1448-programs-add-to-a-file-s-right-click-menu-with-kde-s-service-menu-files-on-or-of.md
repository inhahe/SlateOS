## 1448. Programs add to a file's right-click menu with KDE's service-menu files, on or off by the user

**Date:** 2026-09-29 &middot; **Decided by:** Claude (autonomous) &middot;
**Lane:** C

**In short:** A program can add items -- "Compress", "Rotate right", "Open a
terminal here" -- to the menu a right-click on a file opens, by installing a
small text file that says what the item is called, which kinds of file it is
for and what command it runs. The format is the one KDE's file manager reads,
so a program ported from Linux that ships such a file works here unchanged.
Items a package installed are on until the user turns them off; items dropped
into the user's own folder are off until the user turns them on. Nothing runs
to show the menu: the program starts only when its item is chosen.

**What `design.txt` asks.** "Should apps be able to add to context menus?
Allow it, but with controls": programs must ask to; items load lazily ("don't
load the program just to show the menu"); a settings page lists them and
turns each off; a handler slower than 200 ms is skipped with "loading...".

**The model** (`gui/servicemenus`). The reference is KDE's own code as it
reads these files today -- KIO's `KFileItemActions` and `DesktopExecParser`,
KCoreAddons' `KMacroExpander` and `KShell` -- read, not remembered: a menu
works here if it works there, and does what it does there.

| | |
|---|---|
| The format | a desktop-entry file with `MimeType` (or, in an older menu, its kinds among `ServiceTypes`), `ExcludeServiceTypes`, `Actions` (`_SEPARATOR_` a line) and a `[Desktop Action <id>]` group per item (`Name`, `Icon`, `Exec`); the menu's own `Name`, `Icon`, `Path`; and KDE's keys `X-KDE-Submenu`, `X-KDE-Priority` (`TopLevel`, `Important`), the three `X-KDE-*NumberOfUrls`, `X-KDE-Require=Write`, `X-KDE-Protocol(s)`. No `Type` is asked for, as KDE asks for none |
| Where | `kio/servicemenus/` under each XDG data directory, the user's before the system's; then the older `kservices5/ServiceMenus/`, and `kservices5/` itself for a file marked `KonqPopupMenu/Plugin`, as KDE still reads them. The first file of a name wins; `Hidden=true` in the user's copy removes the system's menu |
| The commands | KDE's reading of `Exec`, ported (`servicemenus::Command`), not the specification's: a line of shell, codes expanded anywhere -- in quotes too, each value quoted for where it stands so a file's name is never read as shell -- `%f` twice allowed, `%d` `%n` `%D` `%N` still meaning folder and name, `" %f"` appended to a line with no file code; split into arguments and started directly, or run by `/bin/sh -c` where it needs a shell; one run per file unless it has `%F %U %N %D`; started in the menu's `Path=` or the file's folder |
| Asking to | a menu under a system data directory -- which only installing a package writes -- is on unless turned off; one in the user's own directory, which any program the user runs can write, is off until turned on. `context-menus.yaml` holds both lists, by file name |
| Lazily | a menu is a declaration read at scan time; no program runs to show one |
| The settings page | every menu found, used or not, is listed by `scan` with why an unused one is not offered (`Scan::skipped`, `ServiceMenu::unusable`) -- for the Settings application's page, lane E's |
| 200 ms | nothing to time: there is no handler, only a declaration, so no menu can be slow to open |
| Conditions | `X-KDE-ShowIfRunning` and `X-KDE-ShowIfDBusCall` name D-Bus services, which this desktop has none of: the menu is not offered at all, and says so -- an item shown where its author meant it hidden is worse than one missing. `X-KDE-AuthorizeAction` names permissions KDE grants unless an administrator withdrew them; none are withdrawn here, so it is granted |
| Kinds of file | as KDE matches them: a MIME type, `type/*`, `all/all`; `all/allfiles`, `allfiles` and `application/octet-stream` for anything but a folder; a type is also what it inherits -- every `text/*` is `text/plain`, and the caller adds the rest (the shell: a text kind is `text/plain`) |
| Where the items go | as KDE's file manager puts them (`Scan::rows`): `TopLevel` menus' items in the file menu itself, after the rest; the rest -- `Important` first, then submenus by name, then the others -- in an "Actions" submenu when they come to more than four rows, else in the menu; items between two lines ordered by id |
| The shell | a right-click on a desktop icon offers the rows after Open, for every selected icon that is a file or folder; choosing one starts one program per run (`ShellAction::LaunchAll`), each in its working directory (`Launch::dir`). The directories are read again at a right-click when a file in them changed, and `context-menus.yaml` when its change is announced |

| Alternative | For | Against |
|---|---|---|
| **KDE's service-menu files** (chosen) | a format ported programs already ship; declarations, so lazy and fast by construction; one file per menu, so the settings page can list and switch each | KDE's keys and its reading of `Exec` are a de-facto standard, defined by KDE's code rather than a specification -- so that code is what was ported; the D-Bus conditions cannot be honoured |
| Their commands read by the specification (`desktopentry::Exec`) | one reading of `Exec` in the tree | the menus in the wild are written for KDE's reading: `sh -c "cd %d && make"`, `convert %f ... %f`, `%f` in quotes -- each refused or, worse, run with a literal `%f` |
| A manifest of SlateOS's own | could carry a capability field and a SlateOS icon scheme | no ported program ships one; the same information in a new spelling |
| Programs register items at run time over IPC | items could depend on the file's contents | the program must be running to show a menu -- the opposite of "lazily" -- and the 200 ms budget exists only because of this design |
| KDE's plugin menus (code loaded into the file manager) | items computed per file | loading a program's code into the shell to draw a menu; no |

**Two copies this retires.** `gui/toolkit/src/context_ext.rs` and
`gui/desktop/src/context_ext.rs` each modelled the run-time registration row
above -- a registry programs would call, a handler timer -- and neither was
reached by anything
(`TD-C-CONTEXT-MENU-EXTENSIONS-ARE-IMPLEMENTED-TWICE-AND-REACHED-NEITHER-TIME`).
A read of both found nothing the declaration model needs: their matching
was by file extension or glob rather than by kind, their menu builders
handed out item ids without a way back to what an id meant, and their
settings panels drew state no file held. They are deleted with the shell's
wiring.

**Where it differs from KDE, deliberately:** `%i` with no icon is nothing
(KDE passes `--icon ''`); `%v` is nothing; `~user` is left as written (KDE
looks the user up); a line of blanks is refused (KDE would run the file);
the choices are per menu file rather than per item (KDE's `kservicemenurc`
is per item); a menu for the Trash is not special-cased, since the shell
offers none for it.

**Revisit if** a program needs items that depend on a file's contents (the
declaration model cannot say that; a new `X-SlateOS-*` key read here is the
place to grow it), if users want single items of a menu hidden (KDE's
per-item switch), or if the package manager gains capabilities -- a
per-package grant to install menus is the finer form of "on unless turned
off".
