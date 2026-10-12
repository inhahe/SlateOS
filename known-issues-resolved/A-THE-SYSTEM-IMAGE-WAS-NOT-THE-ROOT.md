### A-THE-SYSTEM-IMAGE-WAS-NOT-THE-ROOT -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** the disk image holding the installed system was mounted at
`/mnt`, and `/` was a filesystem the kernel built in memory, so every program
looked for `/bin/sh`, `/etc`, `/usr/share/zoneinfo` and `/home` and found
nothing: `popen`, `system` and `#!/bin/sh` scripts failed with `ENOENT`, a
service found one built-in account, and nothing installed on the image could
be started at boot. Lanes B and D (`d-ab-the-booted-system-has-no-bin-sh`,
`d-a-nothing-on-the-system-image-can-be-started-at-boot`).

**Fixed:** just before init, the boot makes the image `/` with
`Vfs::pivot_root` (`kernel/src/main.rs`, `switch_root_to_image`); `/tmp`,
`/proc`, `/dev` and `/sys` stay over it, and the in-memory root goes to
`/.bootfs` and away. The kernel's default service list and programs go onto
the image only where it has none. Design-decisions §1513. Test:
`fs::vfs::self_test_pivot_mounts`, on a tree under `/tmp`.
