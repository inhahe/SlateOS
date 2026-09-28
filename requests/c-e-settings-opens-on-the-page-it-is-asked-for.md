# C → E — Let Settings open on the page it is asked for

**From:** Lane C (`gui/desktop`). **To:** Lane E (`apps/settings`, `apps/launcher`).
**Filed:** 2026-09-25. **Status:** OPEN.

**In short:** the start menu offered "Display Settings", "Network Settings"
and "Sound Settings", which started `/usr/bin/settings --display` and its
neighbours as one file name, so nothing started. The desktop has dropped them
for now; a search for "wifi" finds Settings itself. To bring them back as
results that land on their page, Settings needs a way to be asked for one.
This request is the caller design-decisions 856 asks a feature to have before
it is built: lane C restores the three rows with it as soon as it exists.

## What would help

`settings --page <name>` opening on that page. Two constraints from the
desktop's side:

- **Not `--display`, `--network` or `--sound`.** `--display <address>` is the
  option every SlateOS program takes for the compositor's address
  (`gui/window/src/app.rs`, `Args::parse`), so `settings --display` is today a
  complaint that the address is missing.
- **Stable names**, because they will be written into `startmenu.yaml` when a
  user pins a page, and into saved desktop shortcuts. Lower-case names of
  `SettingsPage` (`display`, `sound`, `network-status`, `wifi`, ...) would do;
  the choice is yours, and the desktop will use whatever you pick.

An unknown page refused with the list of known ones would match how Settings
treats an unknown argument now. `oswindow::app::launch` rejects any leftover
argument, so this means calling `Args::from_env` and `launch_with` yourself,
as its doc describes.

## The same fault in `apps/launcher`

`apps/launcher/src/main.rs` (about lines 1288-1314) carries its own copy of the
three entries, with the same one-string paths.

## What lane C does after

Gives the start menu's entries arguments and identifies them by the whole
command line (`known-issues.md`
`TD-C-THREE-LAUNCHER-ENTRIES-NAME-A-PROGRAM-THAT-CANNOT-EXIST`, step 2), then
restores the three rows with `--page`.

## What happens until it is done

Nothing breaks: the rows are gone, Settings opens on its first page, and the
page is one click away.
