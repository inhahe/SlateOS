## 268. The bootable USB image is built by our own Python, and the boot test can boot it

**Date:** 2026-08-21
**Decided by:** Claude (autonomous) — implementation choices under the scope
`design-decisions.md` §263 already settled with the operator.
**Lane:** A

**In short:** To run SlateOS on the operator's actual PC we need a file that can
be copied onto a flash drive and started by the machine's firmware. We had
never made one: the test setup builds a fake filesystem inside the emulator each
time it runs, which works beautifully for testing and produces nothing you can
carry across a room. This entry records three choices made while filling that
gap — writing the image ourselves in Python instead of installing third-party
disk tools, plugging it into the emulator as a *pretend flash drive* rather than
as an internal disk, and making the guard against writing to the wrong physical
drive a refusal rather than a warning.

### The gap

`scripts/boot-test.sh` passes QEMU `-drive format=raw,file=fat:rw:build/esp`.
That is QEMU's *virtual FAT* — it synthesises a filesystem on the fly from a
host directory. It is the right thing for a harness: there is no image to
rebuild, so there is no stale image to boot by accident, and the edit-boot loop
stays fast. But it means every structure a firmware must parse before reaching
the kernel — protective MBR, GPT, FAT32 BPB, directory entries — was produced by
QEMU and never by us. A defect in any of them was **invisible in the only test
we had and fatal on the only hardware we care about.**

### Decision 1 — write the image in stdlib Python, do not install disk tools

*What changes:* `python scripts/build-usb-image.py` works in a bare checkout;
nothing new has to be installed on any machine.

| Option | For | Against |
|---|---|---|
| **A. `mtools` + `sgdisk`** (or `xorriso`) | Battle-tested; a few lines of shell | Four new hard prerequisites. None is installed here — checked 2026-08-21, all of `xorriso`, `mformat`, `mkfs.vfat`, `sgdisk` are absent |
| **B. Pure-Python builder** ← chosen | No prerequisites at all; deterministic output; the layout is reviewable | ~600 lines we now own, writing structures we never read back |

The deciding argument is not "fewer dependencies" in the abstract. It is that
this project has already been bitten by exactly this failure: the boot test
carries a prerequisite gate written because a missing `limine/` surfaced as a
`cp: cannot stat` *after* a full workspace build. A tool that is absent in a
fresh clone fails late, in a confusing place, on someone else's machine. And
`scripts/create-disk.py` already writes FAT16/FAT32 from scratch in Python for
the FAT driver self-test, so B is the established pattern rather than a novelty.

`scripts/build-iso.sh` is not a third option: it needs `xorriso` (absent), and
an ISO9660 image is read-only, so it could never carry a writable ESP or a
second partition for a rootfs.

**The real cost of B is honest and worth naming:** nothing in this repository
reads a GPT or a FAT32 directory, so a byte-layout bug's error message is a
black screen on a machine in another room. That is the worst feedback loop in
the project. The mitigation is `scripts/test-build-usb-image.py` — a reader
written *independently of the writer*, checking GPT CRCs, both FAT copies,
cluster chains, long-filename entries, `.`/`..`, and file bytes. A test that
re-derived the writer's own arithmetic would agree with it about its mistakes.

### Decision 2 — attach the image as USB storage, not as a disk

*What changes:* under `--usb-image`, QEMU shows the firmware a USB hard drive.

Attaching it as another SATA/virtio disk would have been one word shorter and
would have tested the GPT and FAT32 equally well. It was rejected because the
thing being rehearsed is a **flash drive**, and the firmware path for one is not
the path for an internal disk: a different enumeration, a different boot-option
class, a different position in the boot order. The verification run shows the
distinction paying off immediately —

```
BdsDxe: failed to load Boot0003 "UEFI QEMU NVMe Ctrl SLATE-NVME-1 1" ...: Not Found
BdsDxe: loading Boot0004 "UEFI QEMU QEMU USB HARDDRIVE 1-0000:00:0d.0-2" ...
```

OVMF passed over every other device and *chose the USB disk*, which is precisely
what the operator's firmware will have to do. The cost is nil: the harness
already carries a `qemu-xhci` controller for the USB keyboard.

### Decision 3 — `--usb-image` is opt-in, and rebuilds unconditionally

*What changes:* an ordinary boot test is unaffected; `--usb-image` adds a few
seconds and boots the real bytes.

Making it the default would have put every layer under continuous test, which is
normally the right instinct. Against it: the virtual-FAT path needs no image
rebuild, which is what keeps the edit-boot loop fast, and `--no-stage` soaks
exist specifically to boot *the ESP that is already there*. Two defaults cannot
both be served, and the fast one is used a hundred times more often.

Within `--usb-image`, though, the image is rebuilt **unconditionally**, even
under `--no-stage`. Skipping the rebuild when the tree "looks unchanged" would
reintroduce exactly the stale-image failure that the staging freshness guard
above it already exists to prevent — and the whole point of §263 is that we are
about to trust this image on hardware.

### Decision 4 — the stick writer refuses, it does not warn

*What changes:* `scripts/write-usb-stick.ps1` cannot target an internal disk at
all, by any argument.

Writing a raw image to the wrong disk is the one genuinely irreversible act in
this whole path, and the standing project rule is that irreversible actions are
the only ones that warrant real caution. A warning printed before a destructive
default is a warning that gets skimmed. So: USB bus type only (not overridable),
never the system or boot disk (not overridable), a size cap because a 2 TB "USB"
disk is far more likely an external backup drive than a boot stick (overridable,
explicitly), and the target's model name must be **retyped**. That last guard is
the load-bearing one: a disk *number* is one keystroke away from another disk; a
model string is not.

Rufus in DD mode would have been a legitimate answer and is still documented as
an alternative in `bare-metal-boot.md`. It was not made *the* answer because it
is a download the operator may not have at the moment they need it, and the
guarded script is available in the worktree that already exists.

### What this does not do

The image carries no rootfs, and the kernel has **no USB mass-storage driver** —
`kernel/src/xhci.rs` binds interface class `0x03` (HID) and nothing else, so the
kernel cannot read the stick it booted from. Firmware read the kernel before
`ExitBootServices`, which is why booting works regardless. The honest goal of
the first bare-metal boot is therefore narrow: does the kernel come up on a real
chipset, with a real firmware memory map, real ACPI tables, a real APIC and a
real PCI bus? Everything needing storage is a later trip. Both gaps are recorded
in `bare-metal-boot.md` §6 so they are not rediscovered as bugs.
