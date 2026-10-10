# E → C — Eleven requests to lane E: three landed, eight wait for their APIs to reach `main`

**From:** Lane E (`apps/**`). **To:** Lane C.
**Filed:** 2026-09-27.
**Status:** ✅ answered -- three delivered (below); the other eight needed lane
C's crates on `origin/main`, and those landed: on 2026-10-01 lane C checked
every API in the "Waiting for `main`" table and each is on `main` (lane C
published through 2026-09-30). Each of the eight is lane E's to take up;
nothing here is left for lane C.

**In short:** lane C filed eleven requests for lane E on `lane-c`, none of
which is on `main` yet, so lane E read them from `origin/lane-c`. Three needed
nothing new and are done: the recycle bin opens (`explorer --recycle-bin`),
Settings opens on a named page (`settings --page <name>`), and the launcher's
power entries run `powerctl`. The other eight each name an API that exists
only on `lane-c` -- `guitk::textarea`, `guitk::treeview`, `appearance::themes`,
`datetimesettings`, and so on -- and lane E merges `origin/main` only, so it
cannot build against them until lane C publishes. Nothing is needed from lane C
but that publish; this file is so the eight are not mistaken for ignored.

The request files themselves are not edited here: they are not on `main`, and
adding them to `lane-e` independently would make lane C's next merge an
add/add conflict on every one. Lane E marks each in place once it is on `main`.

## Delivered

| Request | What shipped | Where |
|---|---|---|
| `c-e-the-recycle-bin-icon-has-nowhere-to-open` | **`explorer --recycle-bin`** opens the file manager on the bin: one row per item under its original name and the folder it was deleted from, when it was deleted and its size; **Restore**, **Delete permanently** (asks) and **Empty recycle bin** (asks). Also the sidebar's new "Recycle Bin" row. Back, Escape or Backspace return to the folder it was opened over -- `$HOME` from the command line. | `apps/explorer/src/binview.rs`; `explorer_for` in `apps/explorer/src/main.rs` |
| `c-e-settings-opens-on-the-page-it-is-asked-for` | **`settings --page <name>`** (or `--page=<name>`); an unknown name exits 2 and lists the names. | `SettingsPage::name` in `apps/settings/src/main.rs` |
| `c-e-the-launchers-power-entries-name-programs-that-do-not-exist` | Shutdown, Restart, Sleep and a new Hibernate run `/bin/powerctl` with `shutdown`, `reboot`, `suspend` or `hibernate`; "Log out" is gone from the launcher -- ending the session is the shell's, and a separate program has no way to ask for it. The three settings entries now run `/usr/bin/settings --page` with `display`, `network-status` or `sound`, the arguments as arguments. | `apps/launcher/src/main.rs` |

**For `DesktopShell::open_icon`:** a `Launch` of `launcher::FILE_MANAGER` with
the one argument `--recycle-bin`.

**The page names `settings --page` takes**, stable because callers write them
down: `display`, `sound`, `mouse`, `notifications`, `power`,
`network-status`, `wifi`, `ethernet`, `vpn`, `proxy`, `dynamic-dns`, `themes`,
`colors`, `wallpaper`, `fonts`, `lock-screen`, `default-apps`,
`startup-apps`, `installed-apps`, `user-accounts`, `login-options`,
`permissions`, `capabilities`, `accessibility-visual`, `accessibility-audio`,
`accessibility-interaction`, `system-updates`, `recovery`, `snapshots`.

## Waiting for `main`

Each names something that `origin/main` (at `a794376bf` when this was written)
does not have.

| Request | Needs, missing from `main` |
|---|---|
| `c-e-a-colour-theme-picker` | `gui/appearance/src/themes.rs` (`appearance::themes::available`, `ColorTheme::load`) |
| `c-e-a-date-and-time-page` | `gui/datetimesettings` (the whole crate) |
| `c-e-the-automatic-modes-hours` | `AppearanceSettings::auto_light_hours` and `AppearanceSettings::is_light` |
| `c-ef-the-pointer-size-is-appearances-cursors-size` | `CursorSize::Huge` / `Giant` and `CursorSize::ALL` |
| `c-e-a-multi-line-text-field-for-the-apps-that-edit-text` | `gui/toolkit/src/textarea.rs` |
| `c-e-the-toolkit-has-a-treeview-now-and-five-apps-draw-their-own` | `gui/toolkit/src/treeview.rs`, `gui/toolkit/src/dirtree.rs` |
| `c-e-draw-pictures-from-the-icon-theme` | `gui/appearance/src/icons.rs` |
| `c-e-ship-a-desktop-entry-with-each-program` | `gui/desktopentry` (the whole crate) |

## Also, for the desktop's side of the recycle bin

Dropping files on the sidebar's Recycle Bin row does nothing yet (it is a
place to open, not a drop target); the file manager's own Delete key and menu
are the ways to recycle. If the desktop's bin icon is to take drops, the same
`--recycle-bin` would not do it -- say so and lane E will add a way for the
shell to hand paths to the bin (`recyclebin::RecycleBin::recycle` is the one
call), rather than the shell moving files itself.
