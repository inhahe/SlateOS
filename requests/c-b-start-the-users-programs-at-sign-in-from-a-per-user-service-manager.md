# C → B — start a user's programs at sign-in from a service manager of their own

**From:** lane C. **To:** lane B (the service manager, `init/`, `userspace/`).
**Filed:** 2026-10-12. **Status:** OPEN.

## In short

Nothing starts a program when someone signs in. The operator decided how it
will (C-Q33; `design-decisions/1608-programs-started-at-sign-in-are-the-service-manager-s.md`):
since startup programs depend on each other and on services -- a tray applet
on the tray, many programs on the input-method daemon, a sync client on the
network and the secret store -- they are started by the service manager,
with its ordering, not by a list of the desktop's own. That is Linux's
systemd per-user manager (XDG autostart files become user units ordered
after the graphical session) and macOS's launchd agents.

## What is asked

1. **A service manager instance per signed-in user**, started when a
   session begins and stopped when it ends, running the user's units in the
   order their dependencies give -- the system manager's logic, not a copy.
2. **A target the desktop reaches**, "the graphical session is up", which
   user units order themselves after; and the session's end, which stops
   them.
3. **The user's startup programs as units** of that manager, in a format of
   yours; an XDG autostart file a ported program installs becomes one,
   **off until the user turns it on** (C-Q33's reason: an installed program
   must not be able to make itself start at every sign-in without asking).
4. **A way to list and switch them**, which lane E's `apps/startupmanager`
   and the Settings page will use as their editor.

## What lane C does

The desktop tells the manager when the graphical session is up and when the
user signs out, and is itself one of the session's units, so "log out" ends
the session's programs (`known-issues/TD-C-LOGGING-OUT-LEAVES-THE-USERS-PROGRAMS-RUNNING.md`
waits on the same manager). Tell lane C the call or message to send, or file
the format back here.
