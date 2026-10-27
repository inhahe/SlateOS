## 1575. A physical disk is read-only unless the boot command line names it

**Date:** 2026-10-27 · **Decided by:** Claude (autonomous; the operator's
requirement that the test PC's C: drive is never erased, and that E: may be
wiped, set the goal) · **Lane:** A

**In short:** SlateOS now boots on a real PC through a KVM box (a network
device that shows the PC's screen and works its keyboard and power), and
that PC's own Windows disk must survive every test. So the kernel refuses
every write to a real disk unless the boot command line names that disk:
the disk is listed read-only, and a write or erase to it fails before it
reaches the disk. Disks that exist only in memory or inside an emulator are
unaffected, so nothing changes in the emulator's boot tests. The test PC's
boot entry names only its spare disk, E:.

### The design

- Every block-device driver says what its sectors are on
  (`BlockDevice::medium`, required, so a new driver cannot forget):
  `Scratch` -- a RAM disk, swap's memory tier, a virtio-blk disk a
  hypervisor makes up -- or `Disk(identity)` -- an NVMe or AHCI disk, real
  or emulated, with its model, serial number and (NVMe) EUI-64.
- At registration the registry asks this boot's fence
  (`blkdev::write_fence`, from `blk.writable=` on the command line) whether
  the device may be changed, once. A fenced device is listed read-only, and
  the registry's wrapper (`Accounted`) answers each write and discard with
  `NotPermitted` (`EPERM`, as Linux answers a write to a read-only block
  device) before the driver sees it; reads and flushes go through.
- `blk.writable=` names disks by `eui64:<hex>` (Windows' printed form can be
  pasted as is) or `serial:<text>`; `all` opens every disk. No
  `blk.writable=` names none, and a value with any item that does not parse
  names none: a typo must never widen it.
- Each physical disk's registration line on the serial log gives its
  identity and whether it is writable, so a boot entry can be written from
  a first, read-only boot.

### The alternatives not taken

| Alternative | For | Against |
|---|---|---|
| Fence off only when asked (`blk.fence=1`) | Changes nothing for anyone who does not ask | A boot entry that forgets it, or an image built without it, writes to whatever disk it finds: the failure is silent and the cost is someone's data |
| Tell a real machine from an emulator (CPUID's hypervisor bit) and fence only real ones | No option needed in the emulator | The bit is not a security boundary and QEMU's software CPU need not set it; an emulated NVMe disk is fenced too here, which costs nothing as no boot test writes one |
| Name disks by model | Readable | Two disks of one model share it |
| Fence partitions rather than disks | Finer | The test PC's spare disk is a whole disk; nothing needs partitions yet |

### What it costs

An installed SlateOS will have to name its own disk on its command line.
There is no installer yet; when there is, it writes `blk.writable=` for the
disk it installs to.

### As built

- `kernel/src/blkdev.rs`: `Medium`, `DiskIdentity`, `WriteFence`,
  `DiskName`, `write_fence`, `register_fenced`, the checks in `Accounted`,
  `self_test_write_fence` (an `Integrity` self-test).
- `kernel/src/nvme.rs`: Identify Namespace's EUI-64 kept; the identity.
- `kernel/src/ahci.rs`: the identity from IDENTIFY DEVICE.
- `kernel/src/mm/swap.rs`, virtio-blk, `RamBlockDevice`: `Scratch`.
