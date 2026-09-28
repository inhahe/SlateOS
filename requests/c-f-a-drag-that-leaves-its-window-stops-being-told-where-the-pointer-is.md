# C → F — A drag that leaves its window stops being told where the pointer is: the compositor has no implicit pointer grab

**From:** Lane C (`gui/desktop`, `gui/toolkit`). **To:** Lane F
(`gui/compositor`). **Filed:** 2026-09-25.
**Status:** OPEN — a compositor change; lane C has found it, not fixed it.

**In short:** Press the mouse button on a scrollbar, a slider or a text
selection in any program, drag the pointer outside that program's window, and
the program stops hearing where the pointer is: the thumb freezes at the edge
until the pointer comes back. Every desktop system does the opposite — while
a button is held, the window that took the press keeps getting the pointer,
wherever it goes (X11 and Wayland call this the *implicit grab*). There is a
second, smaller fault beside it: a right- or middle-button release can be sent
to a different window from the one that got the press.

## What the code does (`gui/compositor/src/lib.rs`)

| event | goes to | where |
|---|---|---|
| left press, client area | the window under the pointer, which is also focused | `handle_mouse_button` |
| other press | the window under the pointer, **not** focused | `handle_mouse_button`, the `else` arm |
| move | **the window under the pointer**, whatever is held | `handle_mouse_move`, `window_at(x, y)` |
| release | **the focused window** | `handle_mouse_button`, the `!pressed` arm |

So:

1. **Motion during a drag is lost outside the window.** A client dragging a
   scrollbar thumb, a slider, a column divider, a text selection or an icon
   receives nothing while the pointer is over another window or the desktop,
   and the pointer's position at the release is the first it hears of it.
   The windows it passes over, meanwhile, receive `Move`s for a gesture that
   is not theirs, which lights their hover states.
2. **A non-left release can reach the wrong window.** A right-click on a
   window that is not focused delivers the press to it (the `else` arm does
   not focus) and the release to the focused window — which then sees a
   release it had no press for, and the clicked window never sees its button
   go up.

## What is asked

The standard rule: **from a press delivered to a client window until the last
button is released, every pointer event — motion and releases, of any button
— goes to that window**, in its local coordinates, which may lie outside its
bounds (negative, or past its size). The window under the pointer gets
nothing while the grab lasts. After the last release, routing returns to
"the window under the pointer".

Two details that are easy to get wrong:

- **Keep the grab across buttons.** Press left, press right, release left:
  the right release still belongs to the grabbing window.
- **A window that goes away mid-grab ends the grab.** The release then goes
  nowhere, rather than to whatever window happens to be under the pointer.

Window moves and resizes (`self.drag`) already behave this way, because the
compositor owns those gestures; this asks for the same guarantee for the
gestures clients own.

## Why lane C is asking

The desktop shell is about to let an item be dragged between the start menu,
the taskbar and the desktop (`design.txt`: "drag and drop icons … between
pinned apps, desktop, and start menu entries"). Those are three of the
shell's own surfaces, so motion over any of them reaches the shell — but a
drag that crosses a program's window on the way stops being tracked until it
comes out the other side. The feature does not wait for this; with the grab,
it would behave the same whatever the pointer crosses.

## If the answer is no

Please say so here, and say what a client should do instead — lane C would
then note in the shell that a drag across a window is not tracked, and every
program with a scrollbar has the same limitation to document.
