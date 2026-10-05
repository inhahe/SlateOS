## TD-C-THE-POWER-MENU-LAUNCHED-PROGRAMS-SLATEOS-HAS-NEVER-HAD (lane C, 2026-09-25) -- FIXED the same day

**In short:** the start menu's power menu listed Shutdown, Restart, Sleep, Lock
and Logout, and pressing the first three -- and the last -- launched
`/sbin/shutdown`, `/sbin/reboot`, `/sbin/suspend` and `/usr/bin/logout`.
SlateOS has none of them. The power utility it does have is `powerctl`
(`userspace/powerctl`: `shutdown`, `reboot`, `suspend`, `hibernate`, through
the service manager). Meanwhile the login screen's own power buttons made the
desktop *exit*, logging "no power service to ask".

**Why it lasted:** the power actions were application-database entries, and a
database entry is a path; the menu test asserted that each row launched *its
entry's* path, which it did. Nothing asked whether the path named anything.

**Fixed:** the power menu is the shell's own list, `power::PowerChoice` --
shut down, restart, sleep, hibernate (new), lock, log out. The four power
actions run `/bin/powerctl <subcommand>`; lock runs the lock screen as the
shortcut does; log out returns to the shell's own login screen. The login
screen's buttons run the same commands (`LoginPowerAction::command`) and the
desktop stays up. The five entries left the application database.

**Still needed from other lanes:** `powerctl` in the image
(`requests/c-d-ship-powerctl-in-the-image.md`); `apps/launcher` carries its
own copy of the same five wrong paths (`requests/c-e-the-launchers-power-entries-name-programs-that-do-not-exist.md`).
And logging out does not end the user's programs:
`TD-C-LOGGING-OUT-LEAVES-THE-USERS-PROGRAMS-RUNNING`.
