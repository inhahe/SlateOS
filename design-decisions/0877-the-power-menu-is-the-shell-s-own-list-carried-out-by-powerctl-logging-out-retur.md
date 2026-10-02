## 877. The power menu is the shell's own list, carried out by `powerctl`; logging out returns to the shell's own login screen

**Date:** 2026-09-25 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The start menu's power menu used to list five entries from the
application database, and pressing them started `/sbin/shutdown`,
`/sbin/reboot`, `/sbin/suspend` and `/usr/bin/logout` -- programs SlateOS has
never had, so nothing happened. The login screen's power buttons made the
desktop quit instead. Now both run SlateOS's real power utility, `powerctl`
(shut down, restart, sleep, and the new hibernate); lock starts the lock screen
as its shortcut does; and log out brings back the login screen the desktop
started with.

### The calls

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Where does the power menu's list live? | the shell's own enum, `power::PowerChoice` | entries in the application database, as before | a database entry is a program path and nothing more. Two power actions need arguments (`powerctl reboot`), and log out has no program at all, so the database could not say what they do -- only name a path, which was all there was room to get wrong. |
| What carries out shut down, restart, sleep and hibernate? | `/bin/powerctl <subcommand>`, started like any program | a power request sent by the shell itself | `powerctl` already asks the service manager for an orderly shutdown and falls back to the power syscalls. A second route from the window manager would duplicate that policy. Starting a program is also the one thing the shell already hands out (`take_launches`). |
| The login screen's power buttons? | the same commands, through the same launch queue | have the desktop exit and let whatever started it act | the exit was a placeholder ("no power service to ask") written when `powerctl` was already in the tree. With one queue, the same button does the same thing on either side of a login, and a test holds it so. |
| What does log out do? | brings back the shell's own login screen, built as at start from the same account list | exit the desktop and let a session manager start a greeter | the shell owns the login screen; there is no session manager yet to start another. The cost is real and recorded: the user's programs stay running behind the returning screen (`known-issues.md` `TD-C-LOGGING-OUT-LEAVES-THE-USERS-PROGRAMS-RUNNING`), which the session manager will own when there is one. |
| On a machine nobody can sign in to? | log out does nothing | show a login screen anyway | a screen no account can unlock is a machine that cannot be used (§824), and one function (`ShellSession::greeter`) now applies that rule both at start and at log out. |

### What is not done here

- `powerctl` is not yet in the image's `/bin` (`requests/c-d-ship-powerctl-in-the-image.md`).
- `apps/launcher` carries its own copy of the old five entries (`requests/c-e-the-launchers-power-entries-name-programs-that-do-not-exist.md`).
- "Reboot in safe mode", in the roadmap's list, needs a safe mode to reboot into.
