# Lane E -> lanes C and F: a program can ask to have only one window, and a second start brings that one forward

**Filed:** 2026-10-09 by lane E. **For:** lane F (`gui/window`: `oswindow::app`,
which every program starts through, and the compositor, which knows the
windows), and lane C (the launcher and the dock, which start programs).
**Status:** OPEN.

**In short:** the operator decided (E-Q5, design-decisions §1239) that opening
a program twice must not lose data. For about a dozen programs, a second copy
is a mistake rather than a way of working: backup, System Restore, Disk
Cleanup, the system settings. Those should keep a single window, so that
starting one again brings the running window to the front instead of opening a
second. Every program starts through `oswindow::app::launch("<name>", &mut
app)`, so that is where lane E would ask for this, once, rather than each
program inventing a lock file. Nothing is broken while this waits. Today a
second copy opens, and lane E's own half (programs that save record by record,
so two windows lose nothing) goes ahead without this.

## What lane E needs

1. **A way to say "one window only"** when a program starts: for example a
   `launch_single` beside `launch`, or an option on it.
2. **A second start that finds the first.** When a program that asked for one
   window is already running for this user, the new start does not open a
   window. The running one is raised and focused, and the new start ends,
   successfully.
3. **What the second start was asked to do, handed to the first.** A program
   started with a file to open should open it in the running window. An event
   to the running program with the second start's arguments would do, for
   example `Event::Reopen { args }`.
4. **No stale claims.** A program that crashed must not leave behind a claim
   that blocks its next start. A claim held by the window system, which knows
   when a window's owner is gone, avoids the stale-lock problem a lock file
   has.

Which of these is lane F's (the application frame and the compositor) and
which is lane C's (the launcher and the dock bringing the window forward rather
than starting the program) is yours to divide. Tell lane E the call, and lane E
adopts it program by program.
