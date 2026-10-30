## 1592. The boot medium's firmware is placed at /lib/firmware, read-only, from boot modules

**Date:** 2026-10-30 · **Decided by:** Claude (operator-approved scope) -- firmware for Linux's drivers is part of roadmap 2.1's layer (§1577, §1590); how it reaches a machine booted from a medium with only an ESP is Claude's call · **Lane:** A

**In short:** Many devices -- the test PC's MediaTek WiFi card among them --
need a firmware file loaded into them before they work, and Linux's drivers
look for those files in `/lib/firmware`. The test PC boots SlateOS from a
USB image holding only the boot partition, so there is no disk with
`/lib/firmware` on it. The firmware files now travel on the boot partition
as bootloader modules, and the kernel puts them at `/lib/firmware` when it
starts, where nothing can change them -- as Linux's own early-boot image
(the initramfs) carries a machine's firmware.

### The decision

1. **Firmware rides as Limine modules named `firmware/<path>`.** Each is
   placed at `/lib/firmware/<path>` (`linuxdrv::mount_boot_firmware`, at
   boot after the virtual filesystems), the directory Linux's firmware
   loader searches when no `firmware_class.path` names another -- so a
   driver process needs no argument to find it. A module whose path is not
   one (not UTF-8, empty, or with an empty, `.` or `..` component) or names
   the same file as another is refused, and then nothing is placed.
2. **On a memfs of its own, mounted at `/lib/firmware` and then made
   read-only.** Never written into the root filesystem, which may be a FAT
   disk; and a driver process loads what the boot medium carried, not what
   another process put there. Without a firmware module nothing is mounted
   or made.
3. **The files are pinned, fetched and verified, not committed.**
   `linuxdrv/firmware.pin` names a linux-firmware release tag and each file
   with its sha256; `scripts/linuxfw.py fetch` takes them from kernel.org
   into `build/firmware/` and refuses any other bytes. Each file is a binary
   under its own licence (linux-firmware's WHENCE: MediaTek's are
   "Redistributable"), so it is fetched as the Linux tree is (§1578).
   `scripts/pikvm.py stage` puts them on the medium; the boot test stages
   only a small text file of hello's own, as QEMU has no device needing
   real firmware.

### Alternatives

- **A `bootfs` exposing every module as a file** -- no copy, but a new
  filesystem type for what a memfs and a remount already give; the copy is
  a few MB, once.
- **Writing into `/lib/firmware` on the root** -- no mount, but a FAT root
  disk would be written at every boot, and the directory would stay
  writable.
- **`firmware_class.path` pointing at a writable directory (`/tmp`)** --
  what hello's fixture does, for its own files; for real firmware another
  process could swap the file before the driver reads it.
- **Committing the firmware** -- no fetch, but megabytes of binaries under
  other licences in the history for ever, and every update another copy.

### Checked by

`linuxdrv::self_test`: `firmware_module_path` at its edges at every boot;
the boot test stages `firmware/slateos/hello.fw`, and the self-test finds
it at `/lib/firmware/slateos/hello.fw` with the module's bytes, refuses a
write there, and has the hello process's "bootfw" run load it through
Linux's loader with no `firmware_class.path`. `scripts/linuxfw.py
--self-test` (pin parsing, fetch, a wrong hash refused and nothing left);
`scripts/pikvm.py --self-test` (the medium's layout and limine.conf, and
the test file's text the same in hello.c, boot-test.sh and pikvm.py).
