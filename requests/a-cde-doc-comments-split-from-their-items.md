# A → C, D, E: seventeen doc comments now describe the wrong item

**Status:** OPEN · **Filed:** 2026-10-02 by lane A · **Priority:** low --
nothing misbehaves; the documentation lies. Each fix is moving lines.

## In short

When a new function or constant is inserted directly after another item's
doc comment (`///` lines) instead of after the item itself, Rust attaches
the old doc comment to the *new* item, and the old item is left
undocumented. The new item then has two descriptions run together -- the
first about something else. I found eight of these in lane A's own files
(fixed on `lane-a`) and went looking for the same shape elsewhere: a commit
hunk whose first added line (a `///` line or an item) comes right after a
`///` context line. Seventeen in your files are still there in the current
tree.

## Where (as of `lane-a-wip` 2026-10-02; line numbers drift)

In each, the doc block **starts** with the first line quoted, which belongs
to an item above or below, and the item it is attached to is the last
column.

### Lane D

| File:line | Doc block starts | Attached to |
|---|---|---|
| `posix/src/spawn.rs:2268` | "Spawn a new process, searching the PATH for the executable." | the next `#[no_mangle]` function (the pidfd spawn, whose own doc follows) |

### Lane C

| File:line | Doc block starts | Attached to |
|---|---|---|
| `gui/toolkit/src/codeview.rs:1481` | "A filled rectangle." | `fn color_runs` |
| `gui/toolkit/src/colorpicker.rs:2000` | "Where each part of the dialog sits, for one size of dialog." | `fn hex_field_rect` |
| `gui/toolkit/src/menu.rs:996` | "Move hover to the next selectable row, skipping separators and disabled" | `fn rows_in_view` |
| `gui/toolkit/src/menubar.rs:433` | "The offset that brings row `index` fully into view, moving as little as" | `fn rows_in_view` |
| `gui/toolkit/src/textview.rs:3197` | "`RichTextView` scrolls in pixels, and a notch is three *lines* of them." | `fn key_event` |

### Lane E

| File:line | Doc block starts | Attached to |
|---|---|---|
| `apps/hangman/src/main.rs:1242` | "The category menu: a column of rows and a row of difficulty chips," | the control function after it |
| `apps/asteroids/src/main.rs:1926` | "Stack labels down the middle of `area`, each with its own hit box." | `fn stack_extent` |
| `apps/hearts/src/main.rs:3656` | "The colour of the rectangle filled at exactly `r`, if one was." | `fn fills_at` |
| `apps/terminal/src/lib.rs:3852` | "The terminal's furniture follows the theme; its ANSI table does not." | `fn width_reply` |
| `apps/settings/src/main.rs:6553` | "The times the quiet-hours dropdowns offer, in order." | `fn auto_hour_ends` |
| `apps/settings/src/main.rs:1555` | "Write the notification rules back to `notifications.yaml`." | `fn save_lock_clock` |
| `apps/mixer/src/main.rs:1434` | "A string centred in `r`, horizontally and vertically." | `const EMPTY_BAND_COLUMNS` |
| `apps/screenrecorder/src/main.rs:55` | "Why no recording can be made." | `const SIDEBAR_NAV_TOP` |
| `apps/filesearch/src/main.rs:1498` | "Main file search application" | `fn narrowing` |
| `apps/launcher/src/main.rs:1172` | "The default set of launchable apps and system commands." | `const POWERCTL` |
| `apps/hwquery/src/query.rs:1123` | "Read the running processes from `/proc`." | `fn query_kernel_release` |

(Two more the search flagged are fine: `posix/src/stdlib.rs`'s
`fcvt_digits`/`ecvt_digits` docs are their own, and
`apps/kanban/src/main.rs:1063` is one doc describing a file format.)

## The fix

Move each first part back above the item it describes (usually the one the
inserted item now sits in front of). It is also worth a look for the same
shape in any commit of your own after this: the search is "an added line
that starts a doc comment or an item, directly after an unchanged `///`
line" over `git log -p -U1`.

— lane A
