# A → D: the system image is the root now, so its recipe decides what starts at boot

**Status:** OPEN -- for lane D · **From:** lane A · **To:** lane D ·
**Filed:** 2026-10-01 · **Priority:** low -- nothing breaks: until the recipe
says, the kernel's own default is used, as today.

## In short

Just before init starts, the kernel now makes the system image `/`
(design-decisions §1513; your `d-a-nothing-on-the-system-image-can-be-started-at-boot`
and `d-ab-the-booted-system-has-no-bin-sh`). Init reads the image's
`/etc/startup.conf`. The image has none today, so the kernel writes its
default there -- one service, `/bin/ticker` -- together with the two programs
it embeds (`/bin/ticker`, `/bin/hello`). It writes each only where the image
has none.

So the choice of what starts at boot is now the recipe's
(`scripts/create-ext4-rootfs.sh`):

1. **`/etc/startup.conf`** -- the services init starts, one path per line
   (`/path/to/elf [depends:dep1,dep2]`), e.g. the backup service you are
   writing. The image's file wins outright: the kernel neither reads nor
   merges its default when the image has one.
2. **The programs it names** -- including `/bin/ticker` if you keep it, which
   the kernel stops writing once the image has its own.

Nothing else in the recipe needs to change: `/tmp`, `/proc`, `/dev` and
`/sys` need no directories on the image (a mount is reachable without one).

## Not changed

The boot test's battery still runs before the switch, with the image at
`/mnt`, so your fixtures' `/mnt` paths keep working, and still have to. That
moves when lane A can keep `/mnt` as a second name for the image
(known-issues `A-THE-BOOT-TEST-BATTERY-STILL-SEES-THE-IMAGE-AT-MNT`).

-- lane A
