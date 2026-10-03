### TD-C-THE-RUN-BOX-BROWSE-BUTTON-HAS-NOWHERE-TO-GO — 2026-08-26 — **FIXED 2026-09-03**

**In short:** The Run box (Super+R — type a program's name, press Enter, it
starts) has three buttons: OK, Cancel and **Browse...**. Browse is supposed to
open a file chooser so you can *point* at the program instead of typing its
name, and put the path you picked into the box. Pressing it currently does
nothing at all. The box stays open with whatever you had typed still in it, and
no chooser appears.

**Where:** `gui/desktop/src/run_dialog.rs` posts `RunDialogEvent::Browse` when
the button is pressed; `gui/desktop/src/lib.rs` `DesktopShell::drain_run_dialog`
drops it on the floor. The dropping is deliberate and is commented as such at
the match arm, and `run_box_wiring_tests::the_browse_button_starts_nothing_and_
leaves_the_box_up` pins the current behaviour so it cannot change by accident.

**Why it is dropped rather than answered.** The obvious answer is wrong. There
is a file manager in the tree (`/usr/bin/explorer`), and starting it is one
line — but Browse does not ask for a file manager. It asks for a *file picker*:
a modal chooser whose answer comes **back into the command box**. Starting the
file manager would give the user a window they did not ask for, in front of the
Run box, and leave the command box exactly as empty as before. That is worse
than the button doing nothing, because it looks like it worked.

**What is actually missing.** `guitk::dialog::FileDialog` exists, is tested, and
would serve — but it does not read the filesystem itself. It has to be *fed* a
`Vec<DirEntry>` by whoever embeds it, and `gui/desktop` performs no filesystem
reads anywhere: the shell reads its own YAML config through `appearance` and
nothing else. So wiring Browse means giving the desktop shell a directory-listing
path it does not currently have, which is a larger change than the button.

**The proper fix.**

1. Give the shell a directory read — one function returning
   `Vec<guitk::dialog::DirEntry>` for a path, propagating the I/O error rather
   than swallowing it, with paths handled as bytes (see
   `TD-C-FILEDIALOG-PATHS-ARE-STRINGS-SO-A-NON-UTF-8-FILENAME-OPENS-THE-WRONG-FILE`,
   which this would be the second caller to hit).
2. Host a `FileDialog` on the popup surface, above the Run box, the way the Run
   box is itself hosted above the other popups (`paint_chrome`'s parts array).
   It is modal about keys for the same reason and by the same mechanism.
3. On `DialogAction::Selected`, put the chosen path into the Run box's text
   field — which needs a `set_text`/`replace_text` on `RunDialog`; today the
   field can only be written by keystroke.
4. On cancel, return to the Run box with its text untouched. The text surviving
   the round trip is the whole point: a Browse that cleared the box would be a
   Browse nobody uses twice.

**Severity while open:** low, and visible rather than silent. The button is one
of three ways to do a thing the other two do; a user who presses it sees nothing
happen and types the path instead. Nothing is lost or corrupted.

**Not a regression.** `run_dialog.rs` had no caller of any kind until 2026-08-26
— the button has never worked, because until this commit the box had no way to
be opened.

### Fixed 2026-09-03

Browse now puts a real `guitk::dialog::FileDialog` up over the Run box; picking
a file drops that file's path into the command field and pressing Enter starts
**that** file, byte for byte. Cancel and Escape return to the box with the typed
text untouched.

**Step 1 of the plan above was obsolete and was not done.**
`guitk::dialog::list_directory` already existed — it was added by
`TD-C-FILEDIALOG-PATHS-ARE-STRINGS-SO-A-NON-UTF-8-FILENAME-OPENS-THE-WRONG-FILE`
(`8d2ddc10d`), the entry this one was blocked on — and it already returns
`Vec<DirEntry>` with names carried as `OsString`. It was reused, not rewritten.

**The listing is pulled in from outside the shell rather than read by it.**
`DesktopShell` performs no filesystem I/O anywhere, and that is load-bearing
rather than incidental: it is why all ~3034 of the shell's tests run offline,
and `gui/desktop/Cargo.toml` says so in a comment. Reading the directory from
inside the shell would also have been wrong in a way that is easy to miss —
`read_dir("/")` *succeeds* on a Windows host (it is a drive root), so a shell
unit test would have silently started listing the developer's `D:` drive
instead of failing honestly. So the split follows the wallpaper pattern already
in the crate:

| | wallpaper | Run box chooser |
|---|---|---|
| shell says what it needs | `WallpaperManager` names an image id + path | `DesktopShell::run_browser_wants() -> Option<&Path>` |
| session does the I/O | `ShellSession::refresh_wallpaper_image`, first thing in `paint_background` | `ShellSession::refresh_run_browser`, first thing in `paint_chrome` |
| answer goes back in | image upload | `DesktopShell::set_run_browser_entries(Vec<DirEntry>)` |

`run_browser_listed: Option<PathBuf>` holds **what was last delivered**, and
`run_browser_wants` reports the chooser's current directory only when it differs.
That is deliberately *derived* rather than a "needs listing" flag: navigating
into a subdirectory re-requests automatically, with no navigation path having to
remember to set anything. (See `design-decisions.md` §809.)

**Modality.** `handle_hotkey_inner` and `handle_mouse_inner` both check the
chooser *before* the Run box, so while it is up every key and every click is
its own. One deliberate asymmetry: a press outside the chooser does **nothing**,
where a press outside the Run box dismisses the box. Dismissing on an outside
press would cost the user their navigation for a stray click; Escape and Cancel
are the ways out. `dismiss_popups` closes the chooser *after* draining the Run
box's events, so a Browse that was pending at dismissal cannot leave a chooser
standing over a box that is gone.

**Two type widenings fell out of it**, both mechanical but both real. A path
Browse chose can have no UTF-8 spelling, so it cannot be carried as a `String`
from the chooser to whoever starts the program:
`RunDialogEvent::Execute(PathBuf)`, `ShellAction::Launch(PathBuf)`,
`HotkeyOutcome::launches: Vec<PathBuf>` and `ShellSession::launches:
Vec<PathBuf>`. And because the *field* must stay a `String` — the user types
into it a character at a time — `RunDialog` keeps `command_exact:
Option<PathBuf>` beside the text, validated at use time by comparing
`path.as_os_str().to_string_lossy().trim()` against the field. Validating at use
rather than clearing on edit is what keeps a dozen text-mutating sites from each
having to remember; it is the same shape as `filename_exact` in the chooser
itself.

`RunDialog::browse_start()` answers where the chooser should open — the
directory of what is currently in the field, so a second Browse resumes where
the first left off, and the root for a bare command name like `terminal` that
is not a path at all. It uses the new public `guitk::dialog::parent_of`, which
exposes the dialog's existing byte-level `parent_path`. Public because
`Path::parent` is the wrong tool here for the reason recorded in the entry above
— a SlateOS filename may legally contain `\`, which a Windows host would split
on — and a second hand-rolled "cut at the last slash" would be a second place to
get it wrong.

**Seven tests** in `run_box_wiring_tests`, replacing
`the_browse_button_starts_nothing_and_leaves_the_box_up`, which pinned the
broken behaviour:
`the_browse_button_puts_a_chooser_up_and_leaves_the_box_under_it`,
`choosing_a_name_that_is_not_utf8_starts_that_exact_file`,
`cancelling_the_chooser_leaves_the_typed_command_alone`,
`keys_reach_the_chooser_and_not_the_box_underneath_it`,
`the_chooser_opens_in_the_directory_of_what_is_typed`,
`dismissing_the_box_takes_the_chooser_down_too`,
`a_press_outside_the_chooser_neither_closes_it_nor_reaches_the_desktop`. Plus
`parent_of_splits_a_name_it_cannot_decode` in guitk.

Mutation-checked in two rounds rather than trusted: making `set_command_path`
clear `command_exact` failed exactly one test
(`choosing_a_name_that_is_not_utf8_starts_that_exact_file`, with the predicted
`left: ["/z\u{FFFD}"] right: ["/z\u{d800}"]`), and disabling the chooser's key
interception failed exactly three. Both reverted.

**One consequence found immediately afterwards**, and fixed the same day: the
command *history* was `Vec<String>`, so a browsed path with no UTF-8 spelling was
remembered in its lossy display form and could not be re-run. See
`TD-C-RUN-HISTORY-CANNOT-HOLD-A-PATH-THAT-IS-NOT-UTF-8` below.
