## 1447. Notifications pop up from the shell, beside the pane that holds them

**Date:** 2026-09-29 &middot; **Decided by:** Claude (autonomous), pending
the operator's C-Q32 on the wider question (which of two notification
systems stays) &middot; **Lane:** C

**In short:** When the desktop has something to tell you -- a wallpaper
that would not open, a colour theme that could not be used, a program that
would not start -- it now pops up a small card at the bottom-right corner,
above the taskbar, for a few seconds, instead of only adding it silently to
the notification list behind the bell. The list stays the record; the pop-up
is the moment of attention. Nothing pops up while Do Not Disturb is on,
while the notification list is open, or at the login screen.

**What was there.** The shell's notification pane is the history: the list
behind the tray's bell, with per-program rules and Do Not Disturb, both
wired to the user's settings. Nothing popped up: `DesktopShell::notify`
filed a notification and badged the bell, and that was all. A separate
program, `gui/notifications`, drew toasts -- but nothing starts it, no
program can send to it, and it kept a second history and a second Do Not
Disturb of its own. C-Q32 asks the operator which system stays; this entry
is the recommended answer's first half, built because it is cheap to undo
while no program can send anything yet.

**The model** (`gui/desktop/src/toasts.rs`, `ToastStack`):

| | |
|---|---|
| What pops up | every notification `notify` files, unless focus assist silenced it (`silent` -- "do not show me") or the pane is open (it is in front of the user already) |
| How long it stays | low 4 s, normal 6 s, high 10 s, urgent until closed (`stays_for`); the pointer over the stack holds every toast |
| Where | stacked upward from a margin above the taskbar at the right edge, beside the bell; the newest at the bottom |
| How many | three; a newcomer pushes the oldest out early (the pane keeps it), never an urgent one -- a newcomer waits for one of those to be closed |
| A press on a toast | opens its notification -- read in the pane, and its program asked for when it names one, as a press on its card does |
| Its close button | the toast goes, the notification stays unread in the pane |
| Opening the pane | takes every toast away at once |
| The login screen | nothing pops up; what arrives is filed and waits |
| Motion | slides in and out along the desktop's curves (§1446), 250 ms as designed; the stack closes a gap along the same curve; an arrival stops at its place (a spring's overshoot would be cut by the surface) |

**One layout.** Where each toast is drawn and where a press lands are both
read from `ToastStack::placed`. The separate program drew a leaving toast in
the stack and skipped it when hit-testing, so for a quarter of a second a
press above it landed on the wrong toast -- the drift two descriptions of one
layout always allow.

**Its own surface, the size of the stack.** The shell's menus are drawn on a
full-screen surface, which is how a click outside a menu closes it. Toasts
must not do that: a press beside a toast belongs to the window under it. So
the toasts get a sixth surface, created between the menus' and the
overlays' (a toast pops up over an open menu; a volume report is read over a
toast), moved and sized to the stack's extent -- its toasts, their shadows
and the strip to the screen's edge they slide through -- and unmapped while
there are none.

**The frame clock.** A toast sliding or settling asks for frames; one
sitting still does not -- the moment its time is up is a deadline
(`ToastStack::next_due_in`), armed as the widgets' and the tooltip's are. So
a toast on screen for six seconds costs two slides' frames and one wake-up,
not 360 frames.

| Alternative | For | Against |
|---|---|---|
| **Toasts in the shell** (chosen) | the history, Do Not Disturb and the rules they obey are the shell's already; the shell's own notices pop up with no channel at all | the shell grows by one module and one surface |
| Start the separate program and send it the shell's notices | the pop-ups in a process of their own | a second history and a second Do Not Disturb that ignore the user's settings, kept in step with the shell's over a channel that does not exist |
| Toasts on the overlay surface | no sixth surface | that surface refuses the mouse (§566): a toast could not be pressed or closed |
| Toasts on the menus' surface | no sixth surface | full-screen: every click on the desktop would land on the shell while a toast showed |

**Not yet:** programs cannot send a notification -- the channel is another
lane's (lane F's protocol or lane D's services), to be requested once C-Q32
is answered. A notification's actions beyond the first, and its progress bar,
which the separate program drew, wait for that channel: nothing that can
reach the shell carries them.

**Revisit if** the operator answers C-Q32 with B -- the toast code is what the
separate program would reuse -- or if a user finds three at once too few.
