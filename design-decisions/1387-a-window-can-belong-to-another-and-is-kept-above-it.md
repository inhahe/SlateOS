## 1387. A window can belong to another -- its own program's, or one another program lent it -- and is kept above it

**Date:** 2026-10-10
**Lane:** F
**Decided by:** Claude (autonomous), for the operator's §1415 ("the dialog
is kept above the program that asked, and belongs to it -- F, the window
system") and lane C's
`requests/c-f-the-file-choosers-window-belongs-to-the-program-that-asked.md`.

**In short:** the file explorer now shows every program's Open and Save
window (§1415). To the user that window is the program's dialog, but it is
the explorer's window, and without the window system's help it is an
ordinary one: click the program and the dialog goes behind it, and the
program looks frozen until the user finds it. Now a window can *belong* to
another: it stays above it and rises with it, opens centred over it, is
minimised and restored with it, and gives the keyboard back to it when it
closes. A program can make its own dialogs belong to its windows directly.
For another program's window, that program has to lend it first -- it gets
a secret handle and gives it to the explorer -- so no program can attach a
window of its own to someone else's without being asked.

### The rule

A window belongs to at most one other, in its own band. While it does:

| | |
|---|---|
| Stacking | Above the window it belongs to. Raising either raises the whole family, each window above the one it belongs to and the raised window's branch above its siblings'. |
| Placement | Centred over it when it is attached, kept reachable on screen as a restored window is, and moved to its desktop. |
| Keyboard | Given the keyboard on attaching if the window it belongs to has it (that window's program lent it for this). When it leaves -- closes, hides, is minimised, gives the keyboard back -- the keyboard goes to the window it belongs to first, if that one can take it, before the most-recent rule of §1384. |
| Minimising | Minimising a window minimises everything belonging to it; restoring or activating it brings back what went with it -- not what the user minimised on its own. |
| The parent closes | What belonged to it becomes an ordinary window. |

Naming the parent (`SetParent`): `Parent::Own(id)` -- one of the sender's
own windows; `Parent::Exported(handle)` -- a window another program lent
with `ExportWindow`, which answers sixteen bytes from the kernel's random
source (the same for the same window every time; gone when it closes);
`Parent::None` -- an ordinary window again. Refused: a handle nothing is
lent under, a parent in another band, and a parent that is the window
itself or belongs to it.

### Why each choice

| Question | Decided | Alternative | Why |
|---|---|---|---|
| How another program's window is named | A handle its program lends (Wayland's xdg-foreign: export, then set-parent-of with the imported handle) | The raw window id the program sends in its request -- lane C's first suggestion | With a raw id any program could pin a window over any other's -- an imitation laid over a bank's window and kept there as the user clicks it. Lending is the owner's consent, and costs the owner one request. |
| What "belongs" covers | Stacking, placement, keyboard, minimising, as above | Stacking only | What a dialog is to every desktop: Windows' owned windows and X11 transients do all four. A dialog left on screen when its program is minimised is a dialog for nothing on screen. |
| When the parent closes | The child becomes an ordinary window | Close the child too | The compositor never destroys a client's window; the child's program decides what its window does when what it was for is gone. |
| Bands | Same band only | Any | An application's window belonging to a shell window in front of everything would ride above every application. |
| Handles | One per window, good until it closes | One per export | Bounded by the window count, so a program exporting before every dialog fills nothing. |
| Modality | Not the compositor's | The parent refuses input while its dialog is up | §1415 leaves it to the program, which refuses its own input while the explorer has the request -- as the request says. |
| The taskbar | Unchanged: the child keeps its own entry | Hide windows that belong to others from the taskbar | That needs a field in the window list, which the shell builds by struct literal; a follow-up once the shell can take one. |

### Revisit if

- The shell wants dialogs off the taskbar or grouped under their window: a
  `parent` field in the window list.
- More than the file explorer needs another program's window as a parent
  (a portal for screenshots, printing): the same handle serves.
