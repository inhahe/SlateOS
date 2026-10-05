## 1434. The settings watch runs inside the desktop shell, not as a service of its own

**Date:** 2026-09-28 &middot; **Decided by:** Claude (operator-approved scope:
§1418 asks for a service beside the saving; where it runs is Claude's call)
&middot; **Lane:** C

**In short:** When a program saves its settings, its other open windows
should show the change at once, and a hand edit should show at all. §1418
asked for something that notices a settings file being rewritten and tells
every window. It now runs as a thread of the desktop shell, which starts it
when the session starts. It watches the settings folder and announces each
changed file through the compositor. The plan filed with lane F called for a
small separate program instead. Nothing here starts separate services yet, so
that program would have had no way to be started.

**The alternatives:**

| Option | For | Against |
|---|---|---|
| **A thread in the desktop shell** (chosen) | the shell lives for the whole session, already holds the compositor connection the announcement travels on, and already runs worker threads that wake its loop (`PictureWorker`); nothing new has to start it | tied to the shell: if the shell stops, so do announcements -- though the desktop has then stopped too |
| A separate program, `settingswatch`, the session starts | isolated: a fault in it cannot take the shell down | no mechanism starts a `gui/` service -- the clipboard program is started by nothing (C-Q29) -- so this would first need one, and a second connection to the compositor |
| In the compositor (offered to lane F in the request) | one process fewer | lane F's to build, in the process whose failure takes every window down |

**What stays easy to change:** the watching is a library, `gui/settingswatch`
(the policy is pure and tested apart from the system calls), so a program of
its own is a `main` around `settingswatch::run` once something can start one.

**What it announces, and when:** SlateOS's inotify has no `IN_CLOSE_WRITE`,
so a rename into place (how `settingsfile` saves) and a deletion are
announced at once, a file written in place once it has been quiet for 250 ms,
and after lost events every settings file in the folder. `settingsfile` now
refuses a name the protocol could not announce, with the same rule
(`gui/settingsname`), so every file it writes is one that can be.
