## 809. The Run box's file chooser is handed its directory listing from outside the shell, does not close when clicked past, and checks its remembered path instead of forgetting it

**Date:** 2026-09-03
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the Run box (press Super+R, type a program's name, press Enter) has
a **Browse...** button that until now did nothing. Making it work means putting a
file chooser on the screen, and a file chooser needs to know what files are in a
directory — which means *reading a disk*. The desktop shell has never read a disk
in its life, and that turns out to be worth protecting. Three choices came out of
wiring it up: who reads the directory, what a click outside the chooser should
do, and how the chooser's answer survives the trip back into a text box that can
only hold text.

### Decision 1 — the session reads the directory; the shell only says which one

`DesktopShell` performs no filesystem I/O at all. The only `std::fs` call in the
whole of `gui/desktop/src` outside tests is the wallpaper decode, and it lives in
`ShellSession`, not the shell. That is why all ~3034 of the shell's tests run
offline with no fixture directory, no temp files and no cleanup — a property
`gui/desktop/Cargo.toml` calls out in a comment.

| Option | *What changes* |
|---|---|
| `DesktopShell` calls `list_directory` itself | one fewer method pair; shell tests now touch the developer's real disk |
| **Shell reports the directory, session reads it** (chosen) | the shell stays pure; two methods and one field of plumbing |

The second option was not chosen only for tidiness. On a Windows host,
`read_dir("/")` **succeeds** — `/` resolves to the current drive's root — so a
shell unit test that opened the chooser would have quietly listed whatever is on
`D:`, and passed or failed depending on the machine. A test that reads the
developer's disk by accident is worse than one that cannot read a disk at all,
because it looks like it is testing the chooser.

The shape copies the wallpaper, which had already solved the same problem:

| | wallpaper | Run box chooser |
|---|---|---|
| shell states a need | `WallpaperManager` names an image id + path | `run_browser_wants() -> Option<&Path>` |
| session satisfies it | `refresh_wallpaper_image`, first thing in `paint_background` | `refresh_run_browser`, first thing in `paint_chrome` |
| answer returns | image upload | `set_run_browser_entries(Vec<DirEntry>)` |

A sub-choice inside it: `run_browser_listed: Option<PathBuf>` records **what was
last delivered**, and `run_browser_wants` compares it against the chooser's
current directory. The alternative — a `needs_listing: bool` set whenever the
directory changes — is one byte instead of a path, but it has to be *set* by
every code path that can navigate: double-clicking a folder, the `^` button,
typing a path, and whatever the chooser grows next. Derived state cannot be
forgotten by a path that does not know it exists. The cost is a `PathBuf` per
open chooser and a comparison per frame, which is nothing next to the listing it
guards.

The same split leaves room for a directory that is not local — a network or
package-store listing would be a change in `ShellSession` alone.

### Decision 2 — a press outside the chooser does nothing

The Run box itself is dismissed by a press outside it. The chooser on top of it
is not, and the asymmetry is deliberate.

| Option | *What changes* |
|---|---|
| Outside press dismisses (matching the Run box) | a stray click while reading a long listing throws away however deep you had navigated |
| **Outside press is swallowed and ignored** (chosen) | the chooser stays put; Escape and its Cancel button are the ways out |

What is being protected is different in the two cases. Dismissing the Run box
costs the user a line of typing they can retype. Dismissing the chooser costs
them a *navigation* — several directories deep, reached by reading listings —
and there is no way to get it back but to do it again. The press is still
consumed rather than passed through, so nothing underneath acts on a click the
user aimed at a window that was in the way.

Modality is otherwise total: `handle_hotkey_inner` and `handle_mouse_inner` both
test the chooser before the Run box, so while it is up every key and every click
belongs to it. `dismiss_popups` closes it *after* draining the Run box's pending
events, which is the ordering that stops a Browse pending at dismissal from
leaving a chooser standing over a box that is already gone.

### Decision 3 — the remembered path is validated when used, not cleared when the text changes

The command field is a `String` because the user types into it. A chosen path
may have no UTF-8 spelling, so `RunDialog` keeps `command_exact: Option<PathBuf>`
beside the text. §808 Decision 2 solved the identical problem in the chooser's
Save field by **clearing** the remembered bytes on the first keystroke that
changed the text. This one does the opposite, and the reason is arithmetic:

| | chooser's Save field | Run box's command field |
|---|---|---|
| places the text can change | 2 (backspace, typed character), both via one helper | 10 — cut, paste, backspace, delete, typed character, clear, three history-recall sites, autocomplete accept |
| *What changes* if one is missed | — | the box starts a file the user is no longer pointing at |

With two sites funnelled through one helper, clearing is safe and a test can
drive both. With ten spread across three subsystems — and history and
autocomplete each free to grow more — "every writer must remember" is a rule
that will eventually be broken by a writer added later, and the failure is
silent and points at the wrong file.

So `execute_current` and `browse_start` both use `command_exact` only while
`path.as_os_str().to_string_lossy().trim()` still equals the field. Nothing has
to be remembered; the check is at the one place the value is believed. The cost
is a lossy re-render per use (a short string, twice per Run-box interaction) and
one genuine false positive: if the user types, by hand, the exact `�`-containing
rendering of a path they had browsed to, they get the browsed path rather than
the literal one. That is a name they cannot type, spelled to match one they did
not choose, resolving in favour of the file they actually pointed at.

### Reversing any of these

Decision 1 is the structural one and should not be reversed — the offline test
suite depends on it and the Windows `read_dir("/")` trap is still there.
Decision 2 is two lines in `handle_mouse_inner`, and
`a_press_outside_the_chooser_neither_closes_it_nor_reaches_the_desktop` is the
test that would have to be rewritten to say the opposite. Decision 3 can become §808's shape at any
time by clearing `command_exact` in all ten writers;
`choosing_a_name_that_is_not_utf8_starts_that_exact_file` fails if a writer is
missed only when it is the one the test drives, which is the argument against
doing it.
