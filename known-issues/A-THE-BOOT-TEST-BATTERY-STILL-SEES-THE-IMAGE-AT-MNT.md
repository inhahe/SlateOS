### A-THE-BOOT-TEST-BATTERY-STILL-SEES-THE-IMAGE-AT-MNT -- 2026-10-01 -- OPEN (lane A)

**In short:** the pivot above happens after the boot's self-tests, so the
battery -- every ring-3 fixture the boot test runs -- still sees the image at
`/mnt` and an in-memory `/` with no `/bin/sh`. A fixture that needs the
standard paths cannot check them at boot-test time: lane D's
`services/ctest-stdio` reports its `popen` checks as not run.

**Where:** `kernel/src/main.rs` (the pivot's place in the boot), 190 `/mnt`
paths in the kernel's self-tests, and every lane-D fixture's `BIN "/mnt/bin/"`.

**The fix:** keep `/mnt` as a second name for the image (a bind mount: one
filesystem at two paths, which the VFS cannot do yet), move the pivot to the
start of the boot, then let the fixtures move to the standard paths at their
owners' pace.
