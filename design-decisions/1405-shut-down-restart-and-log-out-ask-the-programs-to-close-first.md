## 1405. Shut down, restart and log out ask the programs to close first

**Date:** 2026-09-26 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** "Shut down" in the start menu used to switch the machine off at
once: a document with unsaved changes was simply gone, and the program holding
it was never asked. Now shut down, restart and log out first ask every open
window to close -- the same request its own close button makes, so a program
with unsaved work can ask whether to keep it -- and happen once they have. If
some are still open after five seconds, a screen lists them, with "Shut down
anyway" and "Cancel". Sleep, hibernate and lock leave the session as it was,
and happen at once, as before.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Which choices wait | shut down, restart, log out (`PowerChoice::ends_the_session`) | every power choice | sleep, hibernate and lock keep the session; there is nothing to close |
| How the windows are asked | `ShellControlAction::Close`, each window open when the choice was made | destroy them | the close button's request: the program is told and can object; a window opened after -- a program's "save changes?" dialog -- is not asked, or the dialog would be dismissed |
| While waiting | a notice on the overlay surface, "Closing programs to shut down..." | a full-screen "shutting down" screen at once | the overlay takes no input, so the dialog a program puts up is reachable; a screen over it would hide the question the wait is for |
| How long before listing | 5 seconds | wait for ever; or none | Windows' figure: long enough for programs to close on their own, short enough that a walked-away user is not left with a machine that stayed on |
| The list | over a dimmed screen, the programs still open (picture and title), "... anyway" and "Cancel"; Escape is Cancel | go ahead silently after a timeout | a program that has not closed may be the one asking about unsaved work; the user decides |
| Cancel | the programs that did close stay closed | reopen them | nothing can reopen them as they were; cancelling stops what has not happened yet |
| The login screen's power buttons | unchanged, at once | the same wait | nobody is signed in to be asked |

### What this does not do

- **Programs without a window are not asked** -- a background service, a
  program that closed its window and kept running. Ending those is the session
  manager's (`known-issues.md` `TD-C-LOGGING-OUT-LEAVES-THE-USERS-PROGRAMS-RUNNING`).
- **The one-click "Shut down" button** the reference draws, with the other
  choices behind a caret, was waiting on exactly this (`todo.txt`); it is the
  next change. *Done the same day:* the start menu's power button reads "Shut
  down" and does it in one click -- through `choose_power`, so every window is
  asked first -- and the caret at its right end (`power_caret_rect`, the
  reference's 30-pixel `aero-sm-power-caret`, its chevron pointing up the way
  the choices open) opens the other choices, lit with the accent while they
  show. The two are drawn as the reference's: two parts of glass, a line round
  each and a pixel between them.
