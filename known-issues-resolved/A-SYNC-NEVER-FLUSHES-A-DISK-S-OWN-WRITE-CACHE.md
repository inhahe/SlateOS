### [A] `sync` and `fsync` never flush a disk's own write cache -- 2026-10-09

**Status:** FIXED 2026-10-09 -- fixed on lane-a-wip 2026-10-09, on main since b083cfeca (lane A's publish of 2026-10-09, boot-tested green at 3d83e0264).

**In short:** when a program saves a file and calls `fsync` (or the system
runs `sync` before switching off), the kernel writes the data out to the
disk -- but most disks first keep what they are sent in a small memory cache
of their own, and write it to the platters or flash a little later. Only an
explicit "flush your cache" command makes them do it now. The kernel never
sent one, so a power cut a moment after a "successful" save could still lose
the save on a real machine. Under QEMU, which the tests use, the emulator
writes everything out when it exits, so the tests could not see this.

**Where it was:** `kernel/src/blkdev.rs` -- the `BlockDevice` trait had
`read_sector`/`write_sector`(`s`) and no flush. `fs::cache::flush` and
`flush_all` wrote the block cache's dirty entries back and stopped there;
`Vfs::sync` (behind `SYS_FS_SYNC`, `fsync`, `sync`) and every ext4 journal
commit called them. The drivers had the command and never sent it:
`ahci.rs` declared `ATA_CMD_FLUSH_EXT` dead code; virtio-blk did not
negotiate `VIRTIO_BLK_F_FLUSH` (so QEMU ran it write-through, syncing its
host file after every write); NVMe never issued Flush.

**The fix (lane-a-wip, 2026-10-09):**

- `BlockDevice::flush`, default `Ok(())` (a RAM disk, a write-through
  device). AHCI sends FLUSH CACHE EXT (FLUSH CACHE without the EXT form)
  when IDENTIFY says the write cache is on; NVMe sends Flush when Identify
  Controller's VWC says a volatile cache exists; virtio-blk accepts
  `VIRTIO_BLK_F_FLUSH` when offered and sends `VIRTIO_BLK_T_FLUSH`.
- `fs::cache::flush(device)` writes the device's dirty blocks back, then
  flushes the device (its lock released first); `flush_all` flushes every
  device (`blkdev::flush_all`). So `fsync`, `sync` and each ext4 journal
  commit -- which calls the cache flush between its writes -- now order
  against the device as Linux's `REQ_PREFLUSH` does.
- The power switches flush every disk too (`power::flush_filesystems`).
- `blkdev::DeviceStats` counts flushes and their time, and `/proc/diskstats`
  reports them in Linux's fields 16 and 17 (flush requests completed, time
  spent flushing), zero before.

Tests: `fs::cache::self_test` (a flush writes the block back and flushes the
device once, counted); `procfs`'s diskstats test (a flush in field 16).
