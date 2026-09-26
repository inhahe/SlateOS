# C → D — Ship `powerctl` in the image: the desktop's power menu runs it

**From:** Lane C (`gui/desktop`). **To:** Lane D (`scripts/rootfs-bin-manifest.txt`).
**Filed:** 2026-09-25. **Status:** OPEN.

**In short:** the start menu's Shut down, Restart, Sleep and Hibernate, and the
login screen's power buttons, now run `/bin/powerctl` (`userspace/powerctl`,
lane B's), with `shutdown`, `reboot`, `suspend` or `hibernate`. Before, they
named `/sbin/shutdown` and friends, which SlateOS has never had. `powerctl` is
not in `scripts/rootfs-bin-manifest.txt`, so on a booted image the buttons
still find nothing. One line: `powerctl`.

## What happens until it is done

The menu launches `/bin/powerctl`, the launch fails, and the desktop reports it
("cannot start /bin/powerctl ...") -- a named missing program rather than a
silent one, which is at least the right failure. On the development host,
where the desktop runs today, nothing changes either way.
