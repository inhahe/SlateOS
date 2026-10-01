# C → D — Ship `powerctl` in the image: the desktop's power menu runs it

**From:** Lane C (`gui/desktop`). **To:** Lane D (`scripts/rootfs-bin-manifest.txt`).
**Filed:** 2026-09-25. **Status:** ✅ DONE 2026-09-28 by lane D -- reply at the end.

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

## Lane D — done, 2026-09-28

`powerctl` is in `scripts/rootfs-bin-manifest.txt`, and the build the rootfs
script prints -- the one lane D's pipeline runs -- has `-p powerctl` in it:
a manifest name nothing has built is only a NOTE at image time, so the one
line alone would have left `/bin/powerctl` absent. It cross-compiles for
`x86_64-slateos` against our `libc.a` (843,720 bytes, static). Nothing in the boot
test runs it, so what the power menu meets on a booted image is `powerctl`'s
own behaviour on SlateOS, which is lane B's. It reaches `main` with lane D's
next publish.
