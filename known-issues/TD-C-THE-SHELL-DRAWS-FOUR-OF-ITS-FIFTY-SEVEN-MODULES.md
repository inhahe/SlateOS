## TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES (lane C, 2026-08-22)

**In short:** The desktop shell crate contains about fifty-seven modules, and
roughly fifty of them are complete, tested user interfaces — a sound panel, a
display panel, a privacy panel, an on-screen volume overlay, a login screen, a
run dialog, a print manager — that **nothing in the running system can put on
screen.** The shell's paint path reaches exactly four of them. The rest are
drawn only by their own unit tests. Separately, `apps/settings` is a *second*,
independent implementation of most of the same panels, and it is the one that
would actually run.

**Update 2026-09-13 (lane C): the shell now has a binary, which is upstream of
this entry's question.**

`ShellSession` was called by nothing but its own tests, so the paint path this
entry measures -- `paint_background` and `paint_chrome`, the only two functions
that hand render commands to a compositor -- ran only under test. `gui/desktop`'s
`desktop` binary now dials, starts the session and pumps it, and the scripted
demo that used to hold that name is `desktop-demo`.

That does not change the count: the paint path still reaches the same four
modules, and which of the other fifty-odd should be on screen is still this
entry's question. What changes is that the answer can now be observed rather
than argued, because there is a running desktop to look at.

**Update 2026-09-14 (lane C): one of the fifty is a taskbar, and it is the
sharpest case in the pile.**

`scripts/scan-orphan-modules.py` had been *clearing*
`gui/desktop/src/taskbar.rs` -- 2 600 lines -- because it declares
`pub struct WindowId`, a name the shell writes hundreds of times and which
`gui/desktop/src/lib.rs` also declares. Both `lib.rs` are aggregators and were
never collected as owners, so the taskbar module looked like the sole owner of
the name, and every window id in the desktop vouched for it. Fixed the same
day; the module is now pinned like the rest.

**What that module is.** "Pinned application shortcuts (persisted to config),
running application indicators with window grouping, drag-to-reorder, drag
into/out of the pinned section to pin/unpin." The shell draws its own taskbar
from `lib.rs` -- `taskbar_rect`, `taskbar_thickness`, `taskbar_button_width` --
and `PinnedApp` appears nowhere in it. So **pinning an application to the
taskbar is a finished, tested feature that no user can reach**, and the
taskbar they do see cannot pin anything.

**Update 2026-09-14, later the same day: it has a caller now.** The shell
uses `TaskbarState::add_pinned`, `remove_pinned`, `pinned_apps` and
`reorder_pinned` -- four of the module's model methods -- for taskbar pinning,
per design-decisions 849. Its *renderer* still has none and is the part 849
says goes. The window-grouping half of its model is blocked on
`TD-C-NOTHING-CONNECTS-A-LAUNCHER-ENTRY-TO-THE-WINDOWS-IT-OPENS`.

So one of the fifty is no longer unreachable, and the way it happened is worth
noting: not by wiring the module up, but by finding the *feature* the user was
missing and discovering the module already implemented it. The other
forty-nine are still best triaged that way round.

**And it has been polished twice while unreachable.** The palette sweep
converted this module's fourteen hard-coded colours to roles -- its own comment
records the work -- and `gui/toolkit/src/menubar.rs`, also unused, was threaded
with a `viewport` argument by the viewport sweep on 2026-09-14, tests and all.
Neither sweep had any way to know. That is the cost of this entry stated
precisely: not just that fifty modules are unreachable, but that careful work
keeps being spent on them, because nothing in the toolchain says which of the
fifty-seven are alive.

**Where:** `gui/desktop/src/session.rs`, `ShellSession::paint_background`
(`:334`) and `ShellSession::paint_chrome` (`:353`), are the only two functions
that hand render commands to a compositor. Between them they call:

| Called | Which module actually draws |
|---|---|
| `self.wallpaper.get_render_commands` | `wallpaper.rs` |
| `self.shell.render_taskbar` | **`lib.rs` inline** — not `taskbar.rs` |
| `self.shell.render_start_menu` | **`lib.rs` inline** — not `launcher.rs` |
| `self.shell.render_alt_tab` | **`lib.rs` inline** |
| `self.shell.render_calendar` | `calendar.rs` |
| `self.shell.render_zone_overlay` | `snap.rs` |
| `self.shell.render_overview` | `overview.rs` |

So four modules — `wallpaper`, `calendar`, `snap`, `overview` — plus whatever
`lib.rs` draws by hand. A grep for `pub fn render` in `gui/desktop/src` returns
**seventy-eight** entry points; six of those are the `lib.rs` ones above, and
of the remaining seventy-two only four are reached.

Note the two rows in bold especially. `taskbar.rs` defines a
`TaskbarUI::render(&mut self, bar_width, bar_height)` and `launcher.rs` an
`AppLauncher::render(&self)`, and `lib.rs` draws its own taskbar and its own
start menu without consulting either. Those two are not merely uncalled; they
are uncalled *while a second implementation of the same surface ships*.

**And `apps/settings` is a third copy of the settings half.**
`apps/settings/src/main.rs` (8,227 lines) declares its own `SettingsPage` enum
— `Display`, `Sound`, `Mouse`, `Notifications`, `Power`, `NetworkStatus`,
`WiFi`, `Wallpaper`, `LockScreen`, `DefaultApps`, `StartupApps`,
`UserAccounts`, `SystemUpdates` and more — and its own `AudioDevice` /
`AppVolume` types. It does not depend on the `desktop` crate at all (`grep -r
'desktop::' apps/settings` finds nothing). Every one of those pages has a twin
module in `gui/desktop/src` with the same name and the same job.

**Why this is worth a file entry rather than a shrug.** Three separate costs,
and the third is the one that bites:

1. *Nothing exercises the panels against a real caller.* Their arguments,
   their sizes and their assumptions about what the shell would pass are
   asserted only by tests written next to them, by the same reasoning that
   wrote the code. A panel that expects a width the shell would never give it
   is not detectably wrong today.
2. *Two implementations drift.* The `desktop` and `apps/settings` sound pages
   already disagree about their data model; nothing forces them together and
   nothing reports when they part.
3. *It silently doubles the cost of every crate-wide change.* The palette
   conversion (`TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`)
   is threading a `&Palette` through fifty modules that nobody draws — and the
   same work will be needed a second time in `apps/settings`, where the count
   is 2,258 constants (`TD-C-EVERY-APPLICATION-CARRIES-ITS-OWN-COPY-OF-THE-PALETTE-TOO`).
   That is not an argument against doing it: leaving the modules frozen
   guarantees the bug returns the moment they *are* wired up, and a module
   converted now is converted once. It is an argument for deciding **which of
   the two implementations survives** before converting the second one.

**This is not `TD-C-THE-SHELL-CAN-DRAW-ITSELF-AND-NOBODY-CAN-ASK-IT-TO`**,
which was resolved on 2026-08-21 and was about the shell having no event loop
at all. The loop exists now and paints every frame. This entry is about what it
paints *into* that loop, which is four modules.

**Proper fix, and it starts with a decision, not with code.** The question is
whether the shell's settings panels or `apps/settings` is the real one:

- **`apps/settings` survives** — then some fifty modules in `gui/desktop/src`
  are dead code and should be deleted, not converted, and this entry closes by
  shrinking the crate by tens of thousands of lines. Against: the shell's
  panels are the better-tested of the two and several have no counterpart in
  `apps/settings` (the OSD overlay, the print manager, the security dialog,
  the login screen, window rules).
- **The shell's panels survive** — then `apps/settings` becomes a thin host
  that depends on the `desktop` crate and draws them, and the duplicated data
  models in `main.rs` go. Against: a settings *application* that links the
  desktop shell inverts the dependency you would expect, and the shell crate
  is already 60 files.
- **Split by kind** — the panels that are genuinely *shell surfaces* (OSD,
  taskbar, start menu, alt-tab, run dialog, security dialog, login screen)
  stay in `desktop` and get wired into `paint_chrome`; the ones that are
  genuinely *settings pages* move to `apps/settings` and the shell copies are
  deleted. This is probably right, and it is the most work.

Whichever is chosen, the wiring for the shell-surface half is small: each is a
`Vec<RenderCommand>` already, and `paint_chrome` already knows how to build a
`RenderTree` from several parts and show or hide the popup surface.

**If never fixed:** the desktop crate keeps growing interfaces that cannot be
seen, every crate-wide sweep pays for them, and the day someone wires one up is
the day they discover what it assumed. Nothing breaks in the meantime, which is
exactly why it has gone unnoticed for so long — a module nobody draws also
never looks wrong.

### Measured 2026-08-25 — `scripts/scan-orphan-modules.py`, and what it found

The paragraphs above counted `pub fn render` by hand. There is now an
instrument: for every library module in lane C's tree, it asks whether any
other file in the *repository* names one of its top-level public items or its
module path, outside tests and bare re-exports. A `pub` item must be named to
be used, so a "no" is sound — which is what makes this answerable at module
scale when the function-level question (`scan-unwired.py`) is not.

**57 island modules, 113,132 lines, out of 200 library modules scanned** — and
**39 of the 57 are in `gui/desktop`, a crate whose `lib.rs` declares 59.**

*(Corrected the same day. The first run of the instrument said 21 / 39,060,
and that was wrong three separate times over: an associated `fn` sharing a
free item's spelling, an item name matched against a same-named type in a lane
that has never heard of this crate, and an identifier inside a string literal
each alibi'd a slice of the list. All three were found by disbelieving a
clearance and checking the module by hand; see commit `26f8eecd5`. The
lesson generalises past this script: **a scan of this kind is not finished
when it runs, it is finished when its clearances survive being disbelieved.**
Do not cite the 21.)*

The 39-of-59 figure is worth pausing on. The paragraphs above reached "four of
its fifty-seven modules are drawn" by hand-counting `pub fn render` call
sites — a completely different method, asking a narrower question. The two
agree: four modules drawn, thirty-nine with no caller of any kind. That
agreement is the best evidence either number is right.

The largest islands:

| Module | Lines | What is in it |
|---|---|---|
| `gui/toolkit/src/modal.rs` | 4,630 | modal dialog / sheet infrastructure |
| `gui/desktop/src/network_settings.rs` | 4,172 | the whole network settings page |
| `gui/desktop/src/osd.rs` | 3,457 | volume/brightness/lock on-screen display |
| `gui/toolkit/src/svg.rs` | 3,393 | an SVG path/transform/colour parser |
| `gui/desktop/src/notif_pane.rs` | 3,374 | the notification pane and quick settings — **reached 2026-08-26, see the repayment log** |
| `gui/toolkit/src/menubar.rs` | 3,337 | the menu bar widget |
| `gui/desktop/src/touchpad.rs` | 2,841 | touchpad gestures and settings |
| `gui/desktop/src/window_rules.rs` | 2,605 | per-application window placement rules |
| `gui/desktop/src/backup_settings.rs` | 2,591 | backup settings page |
| `gui/desktop/src/notification_settings.rs` | 2,539 | notification settings page |
| `apps/procexplorer/src/features.rs` | 2,517 | process-explorer feature set |
| `apps/imageviewer/src/video.rs` | 2,436 | video playback in the image viewer |
| `gui/desktop/src/hotkeys.rs` | 2,431 | global hotkey binding |
| `gui/desktop/src/login_screen.rs` | 2,417 | the login screen |

Two smaller entries are worth naming because they are a *different* failure
from "written but not yet wired":

- `apps/explorer/src/main.rs` writes `mod columns; mod dropzone; mod thumbs;`
  and then never mentions any of the three again — 5,611 lines compiled into
  the binary by an explicit declaration and reachable from nothing.
  (`columns.rs`, 2,372 of those lines, was wired up on 2026-08-25 — see
  "Repayment log" at the end of this entry.)
- `gui/compositor/src/server.rs` (1,334) reaches `lib.rs` only through
  `pub use server::{Disconnect, Server, ServerStats};`, and no file in the
  tree consumes any of the three names. A re-export is plumbing, not a
  caller; it is exactly the shape that makes an island look connected.

**Three of these are duplication, not merely absence** — and that is the part
that bears on the decision in `open-questions.md` → C-Q6:

- `gui/desktop/src/a11y.rs` and `gui/desktop/src/accessibility_settings.rs`
  are two accessibility models **in the same crate**, sharing six type names
  (`ColorFilter`, `MagnifierConfig`, `StickyKeys`, `FilterKeys`, `MouseKeys`,
  `Magnifier`). `a11y.rs` is the unreached one.
- `gui/desktop/src/default_apps.rs` (2,314) and
  `apps/settings/src/associations.rs` (1,748) both model file-type
  associations, and **neither is reachable**. Deleting one does not fix this.
- `gui/toolkit/src/context_ext.rs` (1,605) and `gui/desktop/src/context_ext.rs`
  (2,043) are two files of the same name in two crates sharing four names
  (`ContextMenuExtension`, `ExtensionId`, `build_context_menu`,
  `render_context_menu`), and **neither is reachable** — the desktop copy was
  the module whose false clearance exposed the `member_names` hole. They are
  not even the same design: the toolkit's `ExtensionId` is a newtype
  `pub struct ExtensionId(u64)`, the desktop's is `pub type ExtensionId = u64`.
  The toolkit copy has the lazy-loading and timeout machinery
  (`TimeoutPolicy`, `LoadingEntry`, `check_timeouts`, `loading_placeholder`)
  the desktop copy lacks entirely; the desktop copy has the rendering and
  targeting machinery (`render_context_menu`, `TargetKind`, `ContextTarget`,
  `MenuPosition`, `ExtensionSettingsUI`) the toolkit copy lacks. Between them
  they are one feature, and there is a **third** implementation of the same
  subject in lane A's `kernel/src/fs/contextmenu.rs`.

**And a sub-finding the module scan does not itself report:** six shell
modules serialise settings that nothing ever stores.

| Module | Writer | Reader | Emits |
|---|---|---|---|
| `a11y.rs` | `:1067 to_config_string` | `:1117 from_config_string` | `key=value` text |
| `power.rs` | `:263 to_config_string` | `:313 from_config_string` | `key=value` text |
| `user_accounts.rs` | `:744 to_config_text` | `:758 from_config_text` | pipe records, `USER\|…` |
| `tray_dnd.rs` | `:410 to_config` | `:427 from_config` | a `TrayArrangementConfig` struct |
| `display_settings.rs` | `:746 to_config_text` | **none** | `key=value` text |
| `input_method.rs` | `:426 to_config_text` | **none** | `key=value` text |

Every caller of all ten functions is inside a `#[cfg(test)]` module, and each
has a passing round-trip test — except the last two, which have no reader to
round-trip against and so serialise to a format nothing in the tree can parse.
Three further problems:

1. **`gui/desktop` performs no file I/O at all.** `fs::write`, `fs::read`,
   `File::open` and `File::create` return nothing across the whole crate. It
   reads `appearance.yaml` via `config::load` at `lib.rs:1148` and never
   writes anything, so there is no path by which any of this text could reach
   a disk.
2. **The text formats are not YAML**, which `design.txt` requires for
   configuration. `power.rs` emits `profile=performance\ndim_timeout=…`;
   `user_accounts.rs` emits `USER|` followed by pipe-delimited fields.
3. **`gui/settingsfile` already exists** and does this correctly — atomic
   temp-file-and-rename, comment- and order-preserving splices into the user's
   own file — and `apps/settings` already uses it for two families.

Note `power.rs` is *not* in the island table: `lib.rs` draws its power menu, so
the module is reached. `PowerManager`, `PowerConfig`, `ScreenSaver` and the
config pair inside it still have no caller. A module absent from the table is
not a clean bill of health for its contents.

**C-Q6 WAS ANSWERED ON 2026-09-07** -- design-decisions 815, option C, split
by kind -- and this paragraph went on saying otherwise for a week. A screen you
*open* moves to `apps/settings` and the shell's copy is deleted; something the
desktop *shows* you stays and gets wired. Nothing below is waiting on a
decision.

What it is waiting on is the work, and the shape of that work is the opposite
of what one would guess: `apps/settings` has navigation for 26 pages and
builders for about thirteen, with `DefaultApps`, `StartupApps`, `WiFi`, `Power`
and others falling through to `build_placeholder_page`. The *shell* holds the
real implementations -- `default_apps.rs` is 2,325 lines. So each one is a port
into the Settings app followed by a deletion from the shell, and deleting first
would replace a working panel with a placeholder.

The paragraph below is kept as it was written, because the reasoning in it is
still right about *why* one would not wire a panel that is about to move:

**Do not wire these up before C-Q6 is answered.** Adding `load()`/`save()` to
six models that may be deleted is lesson 45 at a larger size — a bigger unused
feature, with the same round-trip tests making it look covered.

### Repayment log

The ledger is a ratchet, not a monument: `scripts/orphan-modules-baseline.txt`
shrinks as entries are paid off, and `--check` prints "reached now, drop from
the baseline" when one is. Entries C-Q6 does not gate are being paid down while
it sits.

- **2026-08-25 — `apps/explorer/src/columns.rs` (2,372 lines) is reached.**
  The detail view's three hardcoded x-offsets are gone; it now draws through
  `render_column_header` / `render_column_values_from`, so a folder of images
  grows a Dimensions column and a folder of source grows Language and Lines.
  Two defects had to be fixed to make the wiring safe rather than merely
  possible, and both are the same shape as lesson 45 one level down — *a
  provider written for a caller that never arrived is never found out*:
  - `StandardColumns::value` returned `ColumnValue::Empty` for **Size, Date
    Modified, Date Created and Attributes**, with the comment "In a real
    implementation this would stat the file. Return empty as a stub; the
    explorer already has size info." Routing the view through it as-written
    would have silently deleted the two columns the explorer already showed.
    They stat now.
  - `ColumnManager` had only `sort_by`, a three-state header-click toggle
    (asc → desc → **none**). The explorer's own sort is two-state, so no
    number of toggles reliably lands on the state it wants. Added
    `set_sort(id, order)`, with `sort_by` rewritten in terms of it — the
    toggle for a header click, the setter for a caller that already owns the
    sort.

  A third hazard was designed around rather than fixed: every `columns.rs`
  lookup takes a `&str` path, which a name that is not valid UTF-8 cannot
  become without loss. `ExplorerState::row_values` therefore builds the
  standard cells from the `FileEntry` it already has — which also takes a
  per-cell `stat` off the per-frame path — and only falls back to the path for
  columns a *provider* owns. A non-UTF-8 name costs a blank Dimensions cell,
  never a wrong one and never a dropped row.

  Still pinned from this app: `dropzone.rs` (1,259) and `thumbs.rs` (1,980).
  `thumbs.rs` has an obvious home — `ViewMode::Icons` currently renders an
  empty pane, because `render_file_list` only ever drew the `Details` case.

- **2026-08-25 — `apps/explorer/src/thumbs.rs` (1,980 lines) is reached.**
  `render_file_list` is now a three-arm match: `ViewMode::Icons` draws a
  wrapping grid of captioned cells and `ViewMode::List` a single column of
  rows, both through one `push_thumb` so the two cannot drift apart in the one
  decision that matters. Queueing follows the view — switching into a picture
  view fills the queue, switching out empties it — so a folder of ten thousand
  files stops decoding the moment the user stops looking at the pictures.

  Wiring it turned up the same shape of defect one level deeper, and this time
  in the *compositor*: `RenderCommand::Image` had three emitters and a backend
  arm that was a bare `// stub for now`. That is fixed separately (§554,
  `ImageAsset`), and its consequence shapes the explorer's design here — an
  `Image` naming an id the compositor does not hold draws nothing and reports
  nothing, so a thumbnail must be both **generated** and **registered** before
  a command may name it. `ExplorerState` therefore tracks the two states
  separately: `pump_thumbnails` files a result into the cache *and* a pending-
  upload list, `take_pending_uploads` hands them to the host, and only
  `mark_uploaded` makes an entry drawable. An upload that fails, or an image
  the host later drops, falls back to the primitive-only placeholder rather
  than to an empty white frame nobody can diagnose.

  Two defects in `thumbs.rs` itself had to be fixed first:
  - **The generator never touched the disk cache.** `DiskCache` was complete
    and unreachable: `process_batch` called `generate_thumbnail` and nothing
    else, so a restart re-decoded every file. `ThumbnailGenerator` now owns an
    optional `DiskCache` and consults it before generating, populates it after.
  - **The disk cache could not tell two sizes apart.** Its filename was
    `{hash(path, mtime)}.thumb`, with no record of the size cap the entry was
    made at, so a 256px view was served a 64px entry. Fixing it by comparing
    the *stored dimensions* against the cap does not work — `fit_dimensions`
    does not upscale, so a source smaller than the cap yields the same
    dimensions at every cap and every small thumbnail would look stale
    forever. The cap is part of the key instead: `{hash:016x}-{cap}.thumb`,
    with `purge_stale` matching on the hash prefix so a live file keeps its
    entries at *every* cap.

  The in-memory cache gained `peek(&self, …)` beside the promoting
  `get(&mut self, …)`. A renderer reading the cache on every frame must not
  reorder it, or eviction becomes "whatever was last on screen" rather than
  "whatever was last wanted"; `render` taking `&self` is the same guarantee
  spelled by the compiler.

  Still pinned from this app: `dropzone.rs` (1,259).

- **2026-08-25 — `apps/explorer/src/dropzone.rs` (1,259 lines) is reached, and
  `apps/explorer` is off the ledger entirely.** All three of the app's pinned
  islands — `columns.rs` (2,372), `thumbs.rs` (1,980), `dropzone.rs` (1,259),
  5,611 lines — are now called by the app that `mod`-declared them. 54 remain
  repo-wide, from 57.

  A drop zone is a claim about *where something is on screen*, so registration
  belongs to the pass that decides where things go and to no other. `render`
  therefore took `&mut self` and now rebuilds the zone list as it draws: each
  row registers its own rectangle, the sidebar registers its five quick-access
  strips, and the pane registers itself as the fallback. A second pass
  computing the same layout would be a second layout to keep in agreement with
  the first, and it is exactly that kind of shadow layout that goes stale
  without anyone finding out.

  The borrow this needs does not exist: the per-view helpers read
  `self.entries` while writing `self.dropzone`, and a `&self` borrow will not
  split that way. The manager is `mem::replace`d out for the frame and put
  back. That is not only a borrow trick — a *fresh* manager per frame would
  reset `current_hover`, so every frame of a stationary hover would re-fire
  `DragEnter` for a zone the pointer had never left.

  Three defects the wiring exposed:
  - **The module keyed everything by `String`.** Paths, sources, targets: all
    `String`, reached from `PathBuf` by `to_string_lossy`, which maps every
    undecodable unit to U+FFFD and so merges two distinct filenames into one
    target. The same trust-boundary defect class as `columns.rs`'s `&str`
    lookups, and fixed the same way — `Path`/`PathBuf` throughout. The one
    surviving lossy conversion is in `operation_label`, which is drawn for a
    human and never used to reach a file; there, U+FFFD on screen is right and
    hiding the label would be wrong.
  - **The sidebar's rows were their own labels.** They were five strings,
    `"/ (Root)"` among them — harmless while a label was only ever drawn, and
    a drop into a directory literally named `/ (Root)` the moment it was not.
    Label and path are now separate fields of `SIDEBAR_ITEMS`.
  - **A move into the folder the file is already in would have conjured a
    duplicate.** The manager's verdict cannot see this, because it is a fact
    about the *executor*: `fileops`' conflict policy here is `Rename`, so
    moving `notes.txt` into its own directory produces `notes (2).txt` from a
    drag the user meant as a no-op. `ExplorerState::evaluate_drop` wraps
    `handle_drop` and refuses it. A *copy* into the same folder is not the
    same case and still duplicates — people do that on purpose.

  The drop itself runs through `OperationPlan` + `OperationExecutor`, the same
  engine as paste, so a drag-and-drop is undoable, journalled and reports
  per-file errors. A second copy engine behind a drag would have had none of
  those, which is the state paste itself was rescued from.

  **Known limitation, deliberately not fixed here:** `fileops` has no link
  operation, so Alt-drag is *refused* — red feedback and "Links are not
  supported yet" — rather than reported as `Link` and then quietly not
  performed. The proper fix is a `FileOperation::Link` with its own plan,
  journal entry and undo (`unlink` the created link), which is a `fileops`
  feature rather than a wiring one. Until it exists, the refusal is at least
  visible before the user releases the button. See
  `TD-C-A-DRAG-CAN-ASK-FOR-A-LINK-AND-FILEOPS-CANNOT-MAKE-ONE` below.

- **2026-08-26 — `gui/desktop/src/notif_pane.rs` (3,374 lines) is reached, and
  with it the desktop gets its first place to say something the user did not
  ask about.** 53 remain repo-wide, from 54.

  The pane lives on `DesktopShell` and is opened by Super+N, on the pattern
  §493 set for the calendar: the pane's own `PaneState::is_visible` **is** the
  open flag, and there is no second `notifications_open: bool` anywhere. It
  joins `dismiss_popups`, the session's `popups_open`, `anything_moving` and
  `step_frame`, and it is drawn last of the popup parts — its scrim dims what
  is behind it, so drawn earlier it would be the thing dimmed, by its own
  scrim, under a switcher it is meant to be in front of.

  **This was not merely unwired; it was unwireable as written.** `show()` set
  the state to `SlideIn(0.0)`, and both `render` and `handle_mouse_event`
  place the pane's left edge at `screen_width - PANE_WIDTH * visibility`. At a
  visibility of zero that is the right-hand edge of the screen: the pane draws
  nothing, its dimming layer is fully transparent, and a click anywhere is
  judged to have landed *outside* the pane — which the pane treats as "close
  me". Wiring it as-written would have produced a Super+N that appeared to do
  nothing at all. This is the identical defect §520 found in the Exposé
  overlay, and it is now fixed the identical way (§562): the plain verbs land
  on their destination, and a caller that owns a frame clock calls
  `begin_slide()` to rewind the jump into an animation.

  **What the wiring was for.** `ShellSession::wallpaper_error` had four writers
  and exactly one reader — a `pub fn` nothing in the tree called — so a
  wallpaper that failed to decode left the user looking at a plain colour with
  no way whatever to find out why. `set_wallpaper_error` is now the single
  writer and posts a notification when the message is new. The three conditions
  are each load-bearing:
  - *When the message is **new**, not when it is `Some`.* `paint_background`
    runs on every repaint, so an unconditional post files one complaint per
    mouse click for as long as a broken file stays selected.
  - *New **message**, not "is there already an error".* Deduplicating on the
    latter silences every failure after the first, so a user who fixes one path
    and mistypes the next hears nothing about the second.
  - *Posted without **opening** the pane.* A wallpaper that did not load is a
    thing to explain, not an emergency to interrupt with; a panel that shoved
    itself over the screen at login because of a missing file would be worse
    than the missing file.

  Two smaller gaps closed on the way: `PaneState::visibility`/`is_visible` were
  private on a public enum (so no external caller could ask whether the pane
  was open), and the pane had no reader for its own history at all —
  `unread_count` was the whole of its readable state, which is the same reason
  nothing could ask it to draw.

  **Follow-up the same day — the pane now has the mouse route its own doc
  promised.** `notif_pane`'s module comment has said since it was written that
  the pane is toggled "on system tray click or Win+N", and only the chord
  existed. A chord is a route nobody discovers, so the wallpaper message above
  had somewhere to appear and no way to be found. The tray grew a bell in its
  own fixed 24-px slot left of the clock (`bell_rect`,
  `Hit::NotificationBell`), accent-coloured with an unread count when the
  history is not empty.

  Making the bell a *toggle* rather than a one-way door forced a second change,
  recorded as §563: the pane used to be as tall as the display, so it covered
  the bell that opened it and the second press landed on its own opaque column
  and did nothing. `DesktopShell::notification_pane_height()` now returns the
  taskbar's top edge and is the single source for the number given to both
  `render` and `handle_mouse_event`. The bar stays visible and live while the
  pane is open; a press on it that is not the bell closes the pane and is spent
  doing so, exactly as the start menu, the power menu and the calendar already
  behave.

  **Repaid the same day — the events are drained and focus assist is no longer
  an island.** `DesktopShell::apply_pane_events` drains
  `NotificationPane::drain_events` from *wrappers* around `handle_mouse` and
  `handle_hotkey`, not from the branch that forwards to the pane: every path
  out of those functions can have closed the pane — the bell, a press
  elsewhere on the bar, `dismiss_popups` — and each reports a `Closed`, so a
  drain on the forwarding branch alone still leaked one entry per click that
  closed it. `gui/desktop/src/focus_assist.rs` (1,466 lines, the second-largest
  island after `osd.rs`) is now constructed as `DesktopShell::focus`:
  `QuickSetting::DoNotDisturb` and `QuickSetting::FocusMode` are views of its
  four-valued mode via `sync_quick_settings`, `notify` marks an arriving
  notification `silent` when the mode suppresses its app (it still enters the
  history — suppression governs attention, not the record) and counts it on
  the manager, the bell's badge counts `attention_count` rather than
  `unread_count`, and the tray glyph is `effective_mode().icon()`.
  `Notification::action` gained its first reader in the tree: a click on a card
  that names a program now returns `ShellAction::Launch`. See
  `design-decisions.md` §564; 14 new tests.

  **Still open from this thread — three toggles with nothing behind them.**
  `QuickSetting::WiFi`, `QuickSetting::Bluetooth` and `QuickSetting::NightLight`
  still move under the cursor and change nothing outside the pane. They are
  matched explicitly in `apply_pane_events` with an empty arm, so adding a
  variant is a compile error rather than a silent no-op, but there is nothing
  in this process for them to talk to: the first two are the network and
  bluetooth daemons' state and the third is the compositor's gamma ramp, all
  reached over IPC the shell does not hold. The proper fix is a shell-side
  client for each, which is blocked on those services existing; until then a
  case could be made for hiding the three rather than showing dead controls,
  which is a user-visible call and so belongs in `open-questions.md` if it is
  ever pressing. Neither the volume nor the brightness slider in the same block
  is wired either, for the same reason.

  **Still open from this thread — focus assist's automatic half.**
  `FocusAssistManager::evaluate_auto_rules` is never called, so
  `effective_mode()` is today exactly `manual_mode`: the schedule rules
  (`AutoRule::Schedule`), and the ones keyed on the machine being fullscreen,
  presenting, gaming or on battery, do nothing. It needs a wall clock — the
  shell reads one for the taskbar clock, so that half is available — and a
  "the foreground window went fullscreen" signal from the compositor, which is
  not. Wiring only the clock half would make a manager that switches modes on
  schedule but never on a game, which is a worse state than neither, so both
  wait together. Note also that `set_mode(Off)` clears `manual_override`, so
  once auto rules *are* live, turning a switch off while an auto rule is
  asserting a mode will re-assert it on the next evaluation — the manual "off"
  needs to become a distinct suppress-the-rule state at that point, not the
  absence of an override.

  **Still an island next door:** `gui/desktop/src/osd.rs` (3,457 lines), the
  volume/brightness/lock on-screen display, remains uncalled. It was the other
  candidate for the wallpaper message and lost on the merits — an OSD
  auto-dismisses, so a user away from the machine at login would miss the text
  for ever, while a notification persists in a history they can come back to.
  It still needs a caller of its own, for the transient messages an OSD is
  actually right for.

- **2026-10-05 -- the window rules have a home on both sides of the split.**
  Under C-Q6's answer (§815) the rules editor is a screen you open, so it
  belongs to Settings -- and `apps/settings` had no counterpart, which is why
  this entry listed window rules among the shell's panels with nowhere else to
  go. The model, the engine and a file for them (`window-rules.yaml`) moved to
  their own crate, `gui/windowrules`, which Settings can depend on without
  linking the shell; the shell reads the file at start and on every change
  (§1465). The page is requested of lane E
  (`requests/c-e-a-window-rules-page-in-settings.md`); when it lands, the
  shell's `RulesSettingsUI` -- still drawn by nothing -- is deleted rather
  than wired.
