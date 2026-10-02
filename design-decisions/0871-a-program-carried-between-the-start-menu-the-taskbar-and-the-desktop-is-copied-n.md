## 871. A program carried between the start menu, the taskbar and the desktop is copied, not moved; over a window it goes nowhere

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** `design.txt` asks that icons can be dragged "anywhere between
pinned apps, desktop, and start menu entries". They can now: drag a
start-menu row, a pinned taskbar button or a program's desktop icon, and let
go on the taskbar (it is pinned there, at the gap you let go in), on the
desktop (a shortcut appears where you let go), on the start button or among
the start menu's pinned rows (it is pinned to the top of the start menu). A
label follows the pointer naming the program and what letting go will do.
Three choices had real alternatives: a drag *copies* rather than moves; a
drop over another program's window does nothing; and the start menu gained a
pinned section of its own, above the launcher's list, as the place a drop on
it goes. The code is `DesktopShell::carry_target`, `drop_program`,
`finish_start_press` and `render_carry` in `gui/desktop/src/lib.rs`.

### Copy or move

| | A pinned button dragged to the desktop | A desktop program icon dragged to the taskbar |
|---|---|---|
| **Copy** -- chosen | a shortcut appears; the pin stays | the program is pinned; the icon stays |
| **Move** | a shortcut appears; the pin is gone | the program is pinned; the icon is gone |

**Why copy.** It is what Windows does between these three places, and it is
the one of the two under which no drag can lose anything: each place has its
own "remove" (Unpin from taskbar, Remove from desktop, Unpin from Start menu)
that says so in words. Under *move*, rearranging the taskbar with a slightly
high drag would delete a pin. The start menu's launcher list is the source of
every program and cannot be moved out of in any case, so copying is also the
only rule that is the same from all three sources.

### A drop over a window

The shell does not know whether a program under the pointer would accept a
dropped program -- there is no drag-and-drop between programs yet -- so there
are two honest answers and one dishonest one:

- **Nothing happens** -- chosen. The label shows the program's name with no
  "Add to desktop" beneath it, so the user sees beforehand that letting go
  there does nothing.
- **A shortcut on the desktop, behind the window.** Rejected: it appears
  somewhere the user was not pointing, hidden, to be found later.
- **Hand it to the program.** Not yet possible; when programs can accept
  drops this is the case to revisit, and `carry_target`'s `None` for a window
  is where it goes.

Knowing where a window is needed the window list's rectangles, which the shell
received and threw away: `ManagedWindow::frame` keeps them now, and
`DesktopShell::window_at` is its only reader.

### The start menu's pinned section

A drop *on* the start menu has to put the program somewhere the user can see
and undo. The start menu listed only the launcher's programs, in the
launcher's order, so there was no such place. It now has pinned rows above
that list, set apart by a line, written to `startmenu.yaml` and read back at
login; a pinned program is still listed in its own place below, as on every
start menu with pins. Only the pinned rows take a drop and can be rearranged
by dragging: the launcher's list is sorted by the launcher, and a drop onto
it does nothing rather than invent an order the next list rebuild would
discard. The start button is a drop target too -- the only one a desktop icon
or a taskbar button can reach, since the menu is closed while they are being
dragged -- and appends.

### Smaller calls made with it

- **A start-menu row starts its program on the release, not the press** --
  the press cannot yet know whether it is a click or the start of a drag.
  The pinned taskbar buttons and the tray already worked that way.
- **Several program icons dropped together keep their order**, rather than
  each landing in the same gap and so stacking up reversed.
- **A folder or document dropped on the taskbar or the start button is
  refused by staying put**: a taskbar button and a start-menu row each start a
  program, and nothing else.
- **While a desktop icon is over the taskbar, its drop outline is not
  drawn**: letting go there does not move it, and the outline would say it
  does.

### How to reverse

*Move* is one line in each of the three release paths (`finish_start_press`,
`finish_pinned_press`, the icon-release branch of `handle_mouse_inner`):
remove the source after `drop_program`. *Shortcut behind the window* is
deleting `window_at` from `carry_target`.
