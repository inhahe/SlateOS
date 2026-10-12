# C → F — a way for a program to send the desktop a notification

**From:** lane C. **To:** lane F (`gui/remote`, `gui/window`, `gui/compositor`).
**Filed:** 2026-10-12. **Status:** OPEN.

## In short

No program can show "download finished" or "new mail": the desktop pops up
and lists notifications, with Do Not Disturb and the user's per-program
rules, but only its own. The operator decided the desktop shell is the one
notification system (C-Q32, answered A;
`design-decisions/1607-the-desktop-shell-is-the-notification-system.md`), and
C-Q32 said the channel programs send on is asked of you once that was
decided. It is decided.

## What is asked

The window system's connection -- which every program with a window
already holds, and which C-Q29's answer extends to programs without one
(`design-decisions/1605-copy-and-paste-travel-through-the-window-system.md`)
-- carrying a notification from a program to the shell, as it carries the
clipboard:

- **What a program sends:** a title, a body, a priority (low, normal, high,
  urgent), an optional icon name, and optional actions (an id and a label
  each); the compositor stamps *which program* sent it -- its app id from
  its windows, or its executable for one with none -- so a program cannot
  send in another's name. The desktop's `notif_pane::Notification` is the
  shape it becomes; the fields above are what it needs from outside.
- **What comes back:** the id the notification was filed under; and, when
  the user presses it or one of its actions, an event to the sending program
  saying which (so "Reply" or "Open" reaches the program), and that it was
  closed.
- **Who receives it:** the shell, through the same shell-only channel its
  other privileged requests use; with no shell connected, the sender is told
  nobody is there rather than left waiting.
- **Replacing and withdrawing:** a program may update a notification it sent
  (by its id) or take it back.

Size limits as the clipboard's: a title and body bounded, so a program
cannot flood the shell.

## What lane C does once it lands

Files what arrives in the pane under the sending program, applies the
user's rules and Do Not Disturb as for its own, pops it up, and sends the
press and close back; then retires `gui/notifications`, which §1607 retires.
Reply here, or with a notice, when it is on `main`.
