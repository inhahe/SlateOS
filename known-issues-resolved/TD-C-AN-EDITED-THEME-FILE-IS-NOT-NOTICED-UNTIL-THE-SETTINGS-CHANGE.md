## TD-C-AN-EDITED-THEME-FILE-IS-NOT-NOTICED-UNTIL-THE-SETTINGS-CHANGE (lane C, 2026-09-25) -- FIXED 2026-10-06

**Status:** FIXED 2026-10-06 on lane C's branch, on main with lane C's next
publish after its boot test. The last open part -- that whatever edits a
theme in place had to send `ReloadAppearance` itself -- is gone: the
desktop's settings watcher now follows the chosen themes' folders and the
themes directories (`settingswatch::Dependents`,
`appearance::dependency_paths`) and announces any change there as a change
to `appearance`, so a theme edited by the `theme` program, a theme editor or
a text editor reaches every window, each of whose watchers already compares
the theme's file (design-decisions §1483). Tests:
`a_followed_folders_changes_are_its_groups` and, where inotify exists,
`a_followed_folder_is_announced_as_its_group_and_refollowed`
(`gui/settingswatch`); `the_folders_followed_are_the_chosen_themes` and
`the_folders_followed_are_the_directories_and_the_chosen_themes`
(`gui/appearance`). The trigger named below -- the first thing to write a
theme file while it is chosen -- arrived the same day:
`appearance::themes::authoring` and the `theme` program.

**Lane F's part DONE 2026-10-01:** applications now watch through
`appearance::watcher()` too (`gui/window/src/app.rs`, `ThemeWatch::new`; test
`an_edited_theme_file_reaches_the_application`, which fails with the plain
watcher). What remains open is only the last sentence of the status below.

**Status:** the fix below exists: `settingsfile::Watcher::with_dependencies`,
and `appearance::watcher()`, which compares the chosen theme's file as well as
`appearance.yaml`. The shell watches with it. **Open:** applications, whose
watcher is `oswindow`'s -- lane F's one-line change,
`requests/c-f-watch-appearance-through-appearance-watcher.md`. And a watcher
still only looks when a `SettingsChanged` announcement tells it to, so whatever
edits a theme in place must then send `ReloadAppearance`, as the Settings app
does after any save -- a theme editor, when there is one, included.

The entry as it was first written:

**In short:** if you edit the colours in the theme you are using, the desktop
does not show the change until you choose a different theme and back. Choosing
a theme, or changing any other appearance setting, is noticed at once; editing
the theme's own file in place is not. Nobody does that yet except by hand --
there is no theme editor -- which is why this is written down rather than
fixed.

**Where:** `gui/settingsfile/src/lib.rs` -- `Watcher::poll` compares the exact
contents of `appearance.yaml` with the last look and reports nothing when they
match. The shell (`gui/desktop/src/lib.rs`, `appearance_watch`) and every
application (`gui/window/src/app.rs`, `ThemeWatch`) re-read the settings only
when that reports a change, and the theme is read as part of the settings
(`appearance::themes`, design-decisions 874). The compositor does not use a
watcher -- `reload_appearance` reads the file afresh -- so after an in-place
edit and a `ReloadAppearance`, window frames change and the shell and the
applications do not.

**To reproduce:** choose a theme (`theme.colors: nord` with
`~/.local/share/slateos/themes/nord/theme.yaml` installed), change a colour in
that file, then save the appearance settings unchanged. The watchers see the
same `appearance.yaml` and report nothing.

**The fix:** the watcher's idea of "what the file held" has to include the
files the document names. `settingsfile::Watcher` gains a list of dependent
paths derived from each document it reads (a function the settings group
supplies -- for appearance, the chosen theme's `theme.yaml` in whichever
directory `ThemeDirs::find` resolves it to), and compares their contents too.
`appearance` exports the one constructor that wires it; the shell's watcher
(lane C) and `oswindow`'s `ThemeWatch` (lane F) switch to it. **Trigger:** the
Settings app's theme editor (`roadmap-detailed.md` §4.6 Theme Editor), the first
thing that will write a theme file while it is the chosen one.
