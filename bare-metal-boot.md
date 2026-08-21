# Booting SlateOS on real hardware

**In short:** SlateOS has only ever run inside QEMU. This document is the path
to running it on the operator's own PC, from a USB flash drive, without
touching the Windows installation. It covers what to build, what to check in
QEMU first, how to write the stick safely, what the firmware needs to be told,
what you should expect to see, and how to undo it if the machine will not
start. The decision to do this at all is `design-decisions.md` §263.

The one irreversible step is writing the stick, and only if it is written to
the wrong disk. `scripts/write-usb-stick.ps1` exists specifically to make that
mistake unreachable — read [Writing the stick](#3-writing-the-stick) before you
pick a target.

---

## Why this needed new code at all

The boot test hands QEMU `-drive format=raw,file=fat:rw:build/esp`. That is
QEMU's *virtual FAT*: it synthesises a filesystem on the fly out of a host
directory. It is a good choice for a test harness — there is no image to
rebuild and therefore no stale image to boot by accident — but it means that
until 2026-08-21 the project had never produced a disk image at all, and every
structure a real firmware must parse before it can reach the kernel was written
by QEMU rather than by us:

| Structure | Who produced it before | Who produces it now |
|---|---|---|
| Protective MBR | nobody — there was no partition table | `scripts/build-usb-image.py` |
| GPT (primary + backup) | nobody | `scripts/build-usb-image.py` |
| FAT32 BPB / FSInfo / FAT | QEMU, in memory | `scripts/build-usb-image.py` |
| Directory entries, long filenames | QEMU, in memory | `scripts/build-usb-image.py` |

`scripts/build-iso.sh` does not fill the gap: it needs `xorriso`, which is not
installed on this machine, and an ISO9660 image is read-only, so it could never
carry a writable ESP or a second partition for a rootfs.

The new script is stdlib-Python only — no `mtools`, `mkfs.vfat`, `sgdisk` or
`xorriso`. That is deliberate. The boot test already has a prerequisite gate
that exists because a missing `limine/` once surfaced as a `cp: cannot stat`
*after* a full workspace build; adding a fresh set of external tools would
recreate that failure mode in every fresh clone. `scripts/create-disk.py`
already writes FAT16/FAT32 from scratch in Python for the FAT driver
self-test, so this is the project's established approach.

---

## 1. Build the image

```bash
./scripts/boot-test.sh --usb-image
```

That stages `build/esp` exactly as an ordinary boot test does, builds
`build/slateos-usb.img` from it, and then boots *the image* — attached as a
`usb-storage` device on the xHCI controller, which is how firmware will see a
real stick: enumerated over USB, not as a SATA disk.

To build the image without booting it:

```bash
python scripts/build-usb-image.py
```

The image is a pure function of the staged tree: timestamps are pinned, and the
disk GUID, partition GUID and FAT volume serial are derived by SHA-256 from the
content. Identical content therefore produces a byte-identical image, so "did
this actually change?" is answerable by comparing hashes rather than mtimes.
(Verified 2026-08-21: two consecutive builds hashed
`5d53e90954aaf0ba…`.)

## 2. Verify it in QEMU first — this is not optional

QEMU's OVMF is an independent UEFI implementation and Limine is an independent
FAT32 reader. If both accept the image, the odds that a physical firmware
rejects it are small, and you will have learned that at a desk instead of in
front of a machine that will not boot. The lines to look for in
`build/serial-test.txt` are these, from the verified run on 2026-08-21:

```
BdsDxe: failed to load Boot0003 "UEFI QEMU NVMe Ctrl SLATE-NVME-1 1" ...: Not Found
BdsDxe: loading Boot0004 "UEFI QEMU QEMU USB HARDDRIVE 1-0000:00:0d.0-2" from PciRoot(0x0)/Pci(0xD,0x0)/USB(0x1,0x0)
BdsDxe: starting Boot0004 "UEFI QEMU QEMU USB HARDDRIVE ..."
...
Limine 8.7.0 (x86-64, UEFI)
...
limine: Loading executable `boot():/boot/kernel`...
[boot] KASAN zero shadow installed
=== Kernel booting ===
```

That run went all the way: **`BOOT_OK` after 327 s**, against a median of 395 s
for this build on this host (38 debug/uninstrumented boots under TCG), so
booting from a real image over USB costs nothing measurable. Its only two failures were the pre-existing
`posix_spawn_file_actions_init` ones inherited from `main`
(`requests/a-b-posix-spawn-file-actions-init-smashes-the-callers-stack.md`),
identical to the run before it — the image path introduced no regression.

Every layer is exercised there and each one proves something distinct:

- OVMF skipped the NVMe and virtio devices and **chose the USB disk**, so the
  protective MBR and GPT parse.
- It found `EFI/BOOT/BOOTX64.EFI`, so the FAT32 geometry and the root directory
  are right.
- Limine printed its menu, so it read `limine.conf` — whose name needs a long
  filename entry, since `conf` is four characters and 8.3 allows three.
- Limine loaded the 41 MiB kernel, so the cluster chain is right for a file far
  larger than one cluster.

## 3. Writing the stick

**Use the guarded script.** From an *elevated* PowerShell, in the worktree:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/write-usb-stick.ps1
```

With no `-DiskNumber` it lists the machine's disks, marks which are USB, which
is the system disk, and **writes nothing**. Then re-run with the number:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/write-usb-stick.ps1 -DiskNumber 2
```

The guards, none of which is there for decoration:

| Guard | Overridable? |
|---|---|
| Target must be `BusType USB` — an internal NVMe/SATA disk cannot be chosen at all | **no** |
| Target must not be the system or boot disk | **no** |
| Target must be at least as large as the image | **no** |
| Target must be ≤ 128 GB (a larger "USB" disk is more likely an external backup drive) | `-AllowLargeDisk` |
| You must retype the disk's model name exactly | **no** |

The last one is the important one. A disk *number* is one keystroke away from
another disk; a model string is not.

The script takes the disk offline before writing, because a mounted volume
means Windows is holding cached writes that can land *after* ours and corrupt
the filesystem we just laid down.

If you would rather use a familiar tool, Rufus in **DD Image** mode does the
same job and applies a similar removable-device filter. Do **not** reach for a
bare `dd` to `\\.\PhysicalDriveN` — that is the one route with no guard at all.

## 4. Firmware settings

| Setting | Value | Why |
|---|---|---|
| **Secure Boot** | **off** | Limine's `BOOTX64.EFI` is unsigned. With Secure Boot on, the firmware refuses to start it and reports a security violation — which looks exactly like a corrupt stick. |
| **Boot mode** | UEFI (not CSM/Legacy) | The image carries only a UEFI bootloader. |
| **Boot device** | pick the stick from the one-time boot menu | Using the *one-time* menu rather than reordering the boot list means nothing about the machine's normal startup changes. |
| **Integrated graphics** | enable (see below) | Only needed for §263's later half. |

The integrated-graphics setting is separate from booting and can wait. §263's
plan is to write the Intel iGPU driver against the real chip; the i7-8700K's
UHD 630 is present in the package but currently disabled in firmware, and the
monitor is on the discrete NVIDIA card. Enabling the iGPU and moving the
monitor cable to the motherboard's video output is what makes the chip visible.
**Do that as a second trip, after a plain boot has worked** — otherwise a
failure to display anything is ambiguous between "did not boot" and "booted but
drove the wrong output".

## 5. What you should see

1. Firmware POST, then the Limine menu with a single `OS Kernel` entry and a
   three-second countdown.
2. `limine: Loading executable 'boot():/boot/kernel'...`
3. The screen clears and the kernel's own framebuffer console takes over —
   `=== Kernel booting ===`, the memory map, then the self-test battery.

Step 3 is the one that has never been tried outside QEMU. The console is a real
framebuffer console (`kernel/src/console.rs` over `kernel/src/fb.rs`, drawing
through `kernel/src/font.rs`) fed by the framebuffer Limine hands over, so it
does not depend on any driver of ours having initialised. If Limine's menu
appears and then the screen goes black and stays black, the framebuffer handoff
is where to look first.

### There will probably be no serial log

The boot test's entire verification model is serial markers, and a desktop
motherboard generally exposes no DE-9 port. Expect to read results off the
screen and photograph anything interesting. If the board has a COM *header*, a
USB-TTL adapter on it gives the full log and is worth the five minutes.

## 6. Known gaps on the first boot — expect these, they are not new bugs

| Gap | Consequence | Tracked as |
|---|---|---|
| The stick carries no `rootfs.ext4` | 59 Path-Z rungs skip. In QEMU the rootfs is a separate virtio-blk disk; a stick has no equivalent. **Rehearsed — see below.** | this document |
| The kernel has no USB mass-storage driver | The kernel **cannot read the stick it booted from**. `kernel/src/xhci.rs` binds interface class `0x03` (HID) only; there is no bulk-only transport and no SCSI layer. Firmware read the kernel before `ExitBootServices`, which is why booting works anyway. | this document |
| No Intel iGPU driver | Only the Limine-provided framebuffer, at whatever mode firmware chose. No mode setting, no acceleration. | §263 |
| Benchmarks record the accelerator as absent | A bare-metal run's benchmark records are labelled as though no accelerator were in use, which is the *opposite* of the truth. | `known-issues.md` → `TD-A-A-BARE-METAL-RUN-RECORDS-ITS-ACCELERATOR-AS-ABSENT` |

The first two together mean the honest goal of the first bare-metal boot is
narrow and worth stating plainly: **does the kernel come up on a real chipset,
with a real firmware memory map, real ACPI tables, a real APIC and a real PCI
bus?** Everything that needs storage is a later trip.

### Rehearsing the diskless shape: `--no-rootfs`

Until 2026-08-21 the boot test attached `rootfs.ext4` whenever the file existed,
with no way to say "not this time" short of renaming it — which races every
other process in the worktree. So every QEMU run tested a machine strictly more
capable than the one the stick will produce. `--no-rootfs` closes that:

```bash
./scripts/boot-test.sh --usb-image --no-rootfs --no-build
```

Verified 2026-08-21: **`BOOT_OK` after 75 s**, all gates green, 59 Path-Z rungs
skipped and each one named in the log.

**Read that 75 s correctly — it is not an improvement.** The tracking run's
395 s median is dominated by the Path-Z rungs, and this run did not execute
them. For the same reason it is not a *greener* result either, even though it
shows zero failures where the tracking run shows two: the two failures are the
inherited `posix_spawn_file_actions_init` crashes, and they live in rungs that
need `/mnt`. Removing the disk removed the failing tests; it fixed nothing.

That trap is why the run is **tagged as an experiment** in
`bench/boot-history.jsonl` (`--no-rootfs (rootfs.ext4 not attached; /mnt rungs
no-op)`) and excluded from the consecutive-clean streak that four open kernel
issues use as their closure bar. A streak extended by unplugging the disk would
certify nothing. Two other places learned about the flag at the same time, for
the same reason: `check_prerequisites` stops asking for a rootfs this run does
not want, and the skip report stops advising a rebuild of an image you
deliberately unplugged.

What the run *does* establish is the thing worth establishing: with only a
FAT32 ESP present and no second block device anywhere, the kernel still boots
to completion and every gate that does not need storage passes.

## 7. Recovery

**Nothing is installed to the internal disk.** No bootloader is written there,
no NVRAM boot entry is added, and the image is confined to the stick. So:

- **The machine will not boot / hangs on a black screen:** power off by holding
  the power button, remove the stick, power on. Windows starts as before.
- **The firmware boot menu no longer lists the stick:** re-check that Secure
  Boot is off and that the boot mode is UEFI rather than CSM.
- **You changed the boot order instead of using the one-time menu:** enter
  firmware setup and move the Windows Boot Manager back to the top.
- **You moved the monitor cable to the motherboard and see nothing:** move it
  back to the NVIDIA card. If the iGPU was also enabled in firmware, the output
  may have followed it; disabling the iGPU again restores the previous
  behaviour.
- **You are worried the wrong disk was written:** `scripts/write-usb-stick.ps1`
  cannot target a non-USB disk, and refuses the system and boot disks outright.
  If you used another tool instead, stop and check `Get-Disk` before rebooting —
  a Windows disk whose GPT was overwritten is recoverable far more often *before*
  anything else writes to it.

## 8. Status

| Step | State |
|---|---|
| Real GPT + FAT32 image built from the staged ESP | **done** — `scripts/build-usb-image.py`, byte-reproducible |
| Image boots under OVMF as a USB mass-storage device | **done** — verified 2026-08-21: OVMF → Limine 8.7.0 → kernel → `BOOT_OK` in 327 s |
| Independent reader test over the image structures | **done** — `scripts/test-build-usb-image.py`, 6 groups green |
| Image boots in the **diskless shape a stick actually has** | **done** — verified 2026-08-21: `--usb-image --no-rootfs` → `BOOT_OK` in 75 s, tagged as an experiment (see §6) |
| Guarded writer for a physical stick | **done** — `scripts/write-usb-stick.ps1` |
| On-screen progress channel | **exists**, never exercised outside QEMU |
| Recovery procedure | **documented above**, never exercised |
| Boot on the operator's PC | **waiting on the operator being at the machine** |
| Intel iGPU driver written against the real chip | blocked on the row above (§263) |
