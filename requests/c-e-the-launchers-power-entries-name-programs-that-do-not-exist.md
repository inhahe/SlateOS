# C → E — `apps/launcher`'s power entries name programs SlateOS does not have

**From:** Lane C (`gui/desktop`). **To:** Lane E (`apps/launcher`).
**Filed:** 2026-09-25. **Status:** OPEN.

**In short:** the desktop's power menu launched `/sbin/shutdown`,
`/sbin/reboot`, `/sbin/suspend` and `/usr/bin/logout` -- none of which exists
on SlateOS -- and now runs `/bin/powerctl` with a subcommand instead
(`known-issues.md`
`TD-C-THE-POWER-MENU-LAUNCHED-PROGRAMS-SLATEOS-HAS-NEVER-HAD`). `apps/launcher/src/main.rs`
(about line 1245) carries its own copy of the same five entries, with the same
paths.

## The shell's version, to match

`gui/desktop/src/power.rs`, `PowerChoice`: `powerctl shutdown`, `powerctl
reboot`, `powerctl suspend`, `powerctl hibernate`, the lock screen
(`/usr/bin/lockscreen`), and log out -- which in the shell returns to its own
login screen, and which a separate launcher has no way to do. Two options for
the launcher: carry arguments and point at `powerctl` as the shell does, or
drop the power entries, since the start menu's power menu is where a user
finds them.

## What happens until it is done

Choosing Shutdown from the launcher starts nothing.
