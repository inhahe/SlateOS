# C → F — let a window say it goes to the system tray when minimised

**From:** lane C. **To:** lane F (`gui/remote`, `gui/compositor`).
**Filed:** 2026-10-05. **Status:** OPEN.

## In short

`design.txt` (§ taskbar, "a system tray like on Windows") asks that a
*program* can choose to start in the system tray or to go there when it is
minimised, and that a *user* can override any program: always in the tray,
always in the taskbar, or neither. The user's half is done -- a window rule's
`tray` action, carried out by the shell (`design-decisions.md` §1467). The
program's half cannot be: nothing tells the shell that a program wants its
window in the tray, and the program cannot do it itself either, because no
event tells it that its window was minimised.

## What is asked

One flag a program sets on its window, carried to the shell:

- **`WindowSpec`** (`gui/remote/src/control.rs`): a `minimize_to_tray: bool`
  (or whatever name suits), `false` by default -- "when this window is
  minimised, put it in the tray rather than the taskbar". Set at creation is
  enough; a request to change it later is not needed yet.
- **The window list** (`gui/remote/src/window_list.rs`): the flag as a state
  bit (`1 << 5` is free), so `WindowInfo` carries it to the shell. A new
  `WINDOW_LIST_VERSION`, for the reason `5` gives -- a new bit is a new
  vocabulary.
- **`guiwindow`'s builder**: a way to set it, beside `app_id`.

A program that wants to *start* in the tray sets the flag and minimises its
window once it is created (`minimize()` exists).

## Why a flag and not an event

The alternative -- tell a program its window was minimised and let it hide
the window behind an icon of its own -- puts the decision in the program,
where the user's rules cannot reach it: a hidden window is one the shell
cannot tell from a closed one, so "always in the taskbar" could not be made
to hold. With a flag, the decision stays with the shell, where the rules
already are, and a user's `tray: false` simply wins over the program's
`true`.

## What lane C does with it

`DesktopShell::apply_window_list` takes the flag as the default for
`ManagedWindow::to_tray`, with a rule's `tray` -- either way -- overriding
it; then the tray entry, its click and its menu are the ones a rule's
`tray` already gets. Tests for: the program's wish alone, a rule overriding
it each way, and the flag changing nothing for a window that is not
minimised.
