## TD-C-LOGGING-OUT-LEAVES-THE-USERS-PROGRAMS-RUNNING (lane C, 2026-09-25)

**In short:** "Log out" in the start menu's power menu brings back the login
screen, and the next person to sign in finds the previous user's windows still
open underneath. Nothing ends the programs the user started, because nothing on
the desktop knows which they are: that is a session manager's job, and there is
no session manager yet.

**Where:** `gui/desktop/src/session.rs`, `ShellSession::log_out`, which puts
the greeter back and does nothing else.

**The fix:** the session manager -- `userspace/logind` is the start of one --
owns the session's processes; logging out asks it to end them
(`loginctl terminate-session` with the session's id), and the desktop, which
is one of them, is started afresh for the next user. What lane C needs from it
is a way to ask, and the session id to ask about.

**Until then:** the returning screen is the right screen and it does let the
right people in; what it cannot do is clear the desk. A single-user machine --
the common case -- is unaffected in practice.

**Narrowed 2026-09-26 (design-decisions §1405):** log out now asks every
window to close first, as shut down and restart do, and returns to the login
screen once they have -- or when the user says to go ahead without the ones
that did not. What is left is a program with no window: a background process,
or one that closed its window and kept running. That still needs the session
manager.

**Widened 2026-10-05: the desktop's own state is not the signed-in
user's either.** The desktop is one process, started once, whose settings
and data are those of the account it runs as -- `HOME` and the XDG folders
of its environment -- whoever signs in at its login screen. So two people
who share a machine share one desktop: its settings, its pinned programs,
the start menu's recent programs, the Run box's history, and its
notifications, which since 2026-10-05 are kept across a restart
(design-decisions §1468). The second person to sign in can open the
notification pane and read the first one's notifications -- a message's
first line, a calendar entry. On a machine one person uses, the common
case, nothing is wrong. Clearing the pane at log out would not fix it: the
history file is the same file for both, so clearing it would erase the first
person's notifications and keeping it would show them at the next start.
The fix is the same session manager: a desktop started per signed-in user,
in that user's environment, so each has their own folders.
