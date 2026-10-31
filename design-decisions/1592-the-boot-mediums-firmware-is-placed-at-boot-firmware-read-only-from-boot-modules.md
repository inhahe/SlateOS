## 1592. The boot medium's firmware is placed at /.boot-firmware, read-only, from boot modules

**Date:** 2026-10-30 · **Decided by:** Claude (operator-approved scope) -- firmware for Linux's drivers is part of roadmap 2.1's layer (§1577, §1590); how it reaches a machine booted from a medium with only an ESP is Claude's call · **Lane:** A

**In short:** Many devices -- the test PC's MediaTek WiFi card among them --
need a firmware file loaded into them before they work, and Linux's drivers
look for those files in a directory they are told, or else in
`/lib/firmware`. The test PC boots SlateOS from a USB image holding only
the boot partition, so there is no disk with firmware on it. The firmware
files now travel on the boot partition as bootloader modules, and the
kernel puts them in a directory of their own, `/.boot-firmware`, when it
starts, where nothing can change them, and tells every driver process to
look there first -- as Linux's own early-boot image (the initramfs) carries
a machine's firmware.

### The decision

1. **Firmware rides as Limine modules named `firmware/<path>`.** Each is
   placed at `/.boot-firmware/<path>` (`linuxdrv::mount_boot_firmware`, at
   boot after the virtual filesystems). A path is bytes, as every path is
   here. A module whose path is none (empty, or with an empty, `.` or `..`
   component), names the same file as another, or is a directory on
   another's path (or the reverse), is refused, and then nothing is placed.
   A module the kernel cannot read whole -- a name with no end within its
   bound, or one past the most it reads -- is said at boot, since it may be
   firmware a driver will not find.
2. **On a memfs of its own, written before it is mounted at
   `/.boot-firmware`, and read-only as a whole from its first moment**
   (`place_firmware`, `memfs::mount_sealed`; built first since 2026-10-31,
   mounted and then sealed before). The mount point's parent is `/`, which
   always exists and is never renamed or replaced, so placing the firmware
   writes nothing into the root -- which may be a FAT disk, a read-only
   one, or, after the pivot of §1513, the system image -- and no symbolic
   link can stand in for the directory. A placement that fails mounts
   nothing: no firmware rather than some. Without a firmware module nothing
   is mounted.
3. **Every driver process is told `firmware_class.path=/.boot-firmware`**
   (`linuxdrv::start`, unless a run names a directory of its own) -- once
   the firmware is placed, whole and sealed, and only then: without the
   mount the name is anyone's to make on the root. Linux's loader searches
   it first,
   then `/lib/firmware` -- so firmware an installed system carries is still
   found, never hidden by the medium's.
4. **The files are pinned, fetched and verified, not committed.**
   `linuxdrv/firmware.pin` names a linux-firmware release tag and each file
   with its sha256; `scripts/linuxfw.py fetch` takes them from kernel.org
   into `build/firmware/` and refuses any other bytes, leaving no file that
   is not the pin's. Each file is a binary under its own licence
   (linux-firmware's WHENCE: MediaTek's are "Redistributable"), so it is
   fetched as the Linux tree is (§1578). `scripts/pikvm.py stage` puts them
   on the medium; the boot test stages only hello's own test file
   (`linuxdrv/hello/hello.fw`), as QEMU has no device needing real firmware.

### Alternatives

- **At `/lib/firmware`**, Linux's default directory -- the first version.
  No path to pass, but `/lib` had to be made on the root, which may be a
  FAT disk or read-only; an installed system's own firmware there would be
  hidden for the whole session, the mount staying at its path over the
  system image after §1513's pivot; and `/lib`, an ordinary directory, could
  be renamed and replaced by a link to one anyone writes. The reviews of
  2026-10-30 found all four.
- **A `bootfs` exposing every module as a file** -- no copy, but a new
  filesystem type; the copy's cost is
  `known-issues/TD-A-BOOT-FIRMWARE-IS-HELD-TWICE`.
- **`firmware_class.path` pointing at a writable directory (`/tmp`)** --
  what hello's fixture does, for its own files; for real firmware another
  process could swap the file before the driver reads it.
- **Committing the firmware** -- no fetch, but megabytes of binaries under
  other licences in the history for ever, and every update another copy.

### Checked by

`linuxdrv::self_test`: `firmware_module_path` and `firmware_files` at
their edges at every boot (bytes, clashes either way round, a name only
beginning like another's); `place_firmware` at a scratch mount point --
nothing mounted for nothing, two files placed and sealed, a placement
failing part-way leaving no mount; the boot test's
`firmware/slateos/hello.fw` found with the module's bytes, a write beside
it refused, and the hello process's "bootfw" run loading it through Linux's
loader -- and, given no capability to read files, failing its initcall, so
a hello that did nothing cannot pass. `scripts/linuxfw.py --self-test`
(the pin's parsing; fetching with a lost connection, a body cut short, a
server error, each retried; a 404 said at once; a wrong hash refused and a
stale file gone); `scripts/pikvm.py --self-test` (the medium's layout and
limine.conf; the test file's bytes the text hello.c checks).
