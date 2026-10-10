## TD-C-THREE-LAUNCHER-ENTRIES-NAME-A-PROGRAM-THAT-CANNOT-EXIST -- 2026-09-17; the live half FIXED 2026-09-25, the page half FIXED 2026-09-29

**FIXED 2026-09-29 -- the pages are reachable again, and not as rows of
their own.** Two changes, neither the one step 2 below planned. On
2026-09-27 (759c9e9e3, design-decisions §1425) the three pages came back as
Settings' *jump list*: actions of its desktop entry, `settings --page
display`, `network-status` and `sound`, offered by a right-click on Settings.
And on 2026-09-29 (design-decisions §1451) the start menu's search finds a
program's actions by name, ranked with the programs: "display" lists
Settings' "Display settings" first -- above Settings, which it finds only by
a keyword -- and a click or Enter opens the page. That is what the three
rows did, with no row claiming a program of its own, so the identity rework
step 2 feared (four entries sharing `/usr/bin/settings`, twenty sites keyed
by it) is not needed: an action is carried by its program's entry, and is
not pinned or dragged. `launcher::tests::every_settings_page_the_desktop_asks_for_is_one_settings_has`
holds every `--page` any of these start to a name Settings answers to.

**Status, 2026-09-25.** This entry was wrong about one thing that mattered:
the three rows *were* live. The start menu has listed the database's
`Category::Setting` entries beside its applications since 2026-08-21
(`DesktopShell::start_menu_entries`), so "Display Settings", "Network
Settings" and "Sound Settings" were start-menu rows -- and search results --
that started nothing at every press, not inert launcher data. Found again
while fixing the power menu's `/sbin/shutdown`
(`TD-C-THE-POWER-MENU-LAUNCHED-PROGRAMS-SLATEOS-HAS-NEVER-HAD`), by checking
every path the database names against the binaries the workspace builds.

*Fixed:* the three entries are gone and their search words are Settings'
own, so "wifi" or "volume" finds Settings, which opens. That respects the
reasoning below -- no row claims a page it cannot open, since there is one
row and it claims only Settings. And
`launcher::tests::every_program_the_menus_start_is_one_this_workspace_builds`
now holds every program the start and power menus start to a binary this
workspace builds, by a path with no space in it: the check that would have
caught this, the power menu's paths, and the screenshot shortcut's flags.

*Still open:* rows that land on their page, in the order below. A second
reason for step 1's design: the flag cannot be `--display`, which is the
compositor address every SlateOS program takes (`oswindow::app::Args`). Step
1 is filed as `requests/c-e-settings-opens-on-the-page-it-is-asked-for.md`,
which is the caller design-decisions 856 asks for. Step 2 has grown since
this was written: four entries would share the program `/usr/bin/settings`,
so everything that identifies an entry by `executable_path` -- pins and
`startmenu.yaml`, the search's de-duplication, drag payloads, desktop
shortcuts -- must identify it by the whole command line instead (about
twenty sites in `gui/desktop/src/lib.rs` and `launcher.rs`).

The entry as first written:


**In short:** the search launcher's built-in app list has three entries
— "Display settings", "Network settings", "Sound settings" — whose
program is written as `/usr/bin/settings --display` and so on. That is one
string naming a file with a space in it, which no filesystem holds. Nothing
runs it today, so nothing is broken yet; the day the launcher is wired up,
all three fail. And even spelled correctly they would do nothing, because the
Settings application never reads its arguments.

**Date:** 2026-09-17. **Lane:** C.

**Where.** `gui/desktop/src/launcher.rs`, the default app database:

    executable_path: "/usr/bin/settings --display".to_string(),
    executable_path: "/usr/bin/settings --network".to_string(),
    executable_path: "/usr/bin/settings --sound".to_string(),

The field is called `executable_path`, is a `String`, and ends up in
`LauncherAction::Launch(path)`.

**Why it is not live.** `LauncherAction::Launch` has no consumer outside its
own module — the dangling end `TD-SHELL-HAS-NOWHERE-TO-SEND-A-LAUNCH`
records. The start menu's path (`ShellAction::Launch`) *is* wired and does
spawn, which is how the identical defect in the screenshot shortcuts was a
live failure at every press rather than a latent one.

**It is wrong twice over.** Even passed as a proper argument, `--display`
would change nothing: `apps/settings` never looks at `std::env::args`. So the
three entries promise a settings page and would open the front page. A
launcher row that says "Display settings" is a claim about where it lands,
which is the shape design-decisions 856 is about.

**Proper fix, and what has to exist first.** Two pieces, in this order:

1. `apps/settings` accepts a page argument and opens that page. It already has
   a `SettingsPage` enum and a `current_page`, so this is small — but it
   should not be built until something passes one, or it is a feature with no
   caller, which is the other half of 856.
2. The launcher's model carries arguments, as `gui/desktop/src/hotkeys.rs`'s
   `Launch { program, args }` now does for shortcuts. One type for "a program
   and how to invoke it" rather than a second one here.

**Trigger.** Do this when `LauncherAction::Launch` gains a consumer. Until
then the three rows are inert and the cheap half-fix — deleting the
arguments so they honestly read `/usr/bin/settings` — is deliberately not
taken, because three rows that all open the same front page is a *worse* lie
than three rows that do not work: the first looks like a feature that works.
