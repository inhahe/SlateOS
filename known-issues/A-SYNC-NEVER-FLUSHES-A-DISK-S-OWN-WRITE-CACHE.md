### [A] `sync` and `fsync` never flush a disk's own write cache -- 2026-10-09

**Status:** OPEN

**In short:** when a program saves a file and calls `fsync` (or the system
runs `sync` before switching off), the kernel writes the data out to the
disk -- but most disks first keep what they are sent in a small memory cache
of their own, and write it to the platters or flash a little later. Only an
explicit "flush your cache" command makes them do it now. The kernel never
sends one, so a power cut a moment after a "successful" save can still lose
the save on a real machine. Under QEMU, which the tests use, the emulator
writes everything out when it exits, so the tests cannot see this.

**Where:** `kernel/src/blkdev.rs` -- the `BlockDevice` trait has
`read_sector`/`write_sector`(`s`) and no flush. `fs::cache::flush` and
`flush_all` write the block cache's dirty entries back and stop there;
`Vfs::sync` (behind `SYS_FS_SYNC`, `fsync`, `sync`) calls them. The drivers
have the command and never send it: `ahci.rs` declares `ATA_CMD_FLUSH_EXT`
(0xEA) `#[allow(dead_code)]`; virtio-blk does not negotiate
`VIRTIO_BLK_F_FLUSH` or send `VIRTIO_BLK_T_FLUSH`; NVMe never issues its
Flush command (opcode 0x00).

**How to see it:** read the code paths above; no boot can show it (QEMU's
default `cache=writeback` persists everything at exit).

**The fix:** a `flush(&mut self) -> KernelResult<()>` on `BlockDevice`,
default `Ok(())` for devices with no volatile cache (RAM disks, the
in-memory test devices), implemented by AHCI (FLUSH CACHE EXT), virtio-blk
(negotiate `VIRTIO_BLK_F_FLUSH`; send `VIRTIO_BLK_T_FLUSH` when the device
offers it, nothing when it does not -- then it has no volatile cache to
flush) and NVMe (Flush, when the Identify Controller `VWC` bit says a
volatile write cache is present). `fs::cache::flush` calls it after writing
back the device's dirty blocks, so `fsync` and `sync` mean what POSIX says,
and the power-off and reboot paths get it through `Vfs::sync`. Count the
flushes in the block layer's accounting (`blkdev::DeviceStats`, which
`/proc/diskstats` already reports as fields 15-17, "flush requests") and
test with a counting device that every `sync` reaches it.
