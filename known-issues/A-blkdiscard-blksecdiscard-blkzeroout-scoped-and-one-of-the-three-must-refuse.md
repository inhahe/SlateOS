### [A] BLKDISCARD/BLKSECDISCARD/BLKZEROOUT: scoped, and one of the three must refuse -- 2026-09-21
**Status:** SCOPED, not written. Lane B asked in `requests/b-a-blkdiscard-needs-blkdiscard-or-it-stays-a-zero-fill.md`.

**In short:** the `blkdiscard` tool used to print that it had destroyed a disk
and destroy nothing. Lane B made it real where it could and made the rest
**refuse**, naming the kernel call it needs. That call is mine.

**There is a real discard path to dispatch to.** `blkdev.rs` defines
`supports_discard()` and `discard(start_sector, count)` on the `BlockDevice`
trait, so this is not a case of returning `EOPNOTSUPP` and calling it honest.

**The three are not one change, and that is the point:**

| ioctl | plan |
|---|---|
| `BLKDISCARD` (0x1277) | real: check `supports_discard()`, bytes to sectors, call `discard` |
| `BLKZEROOUT` (0x127F) | real, but a *different* operation: write zeros. Must not share the discard arm |
| `BLKSECDISCARD` (0x127D) | **must refuse** with `EOPNOTSUPP`. No device here offers a secure-erase guarantee, and a secure discard that is silently an ordinary discard is the exact defect lane B refused to ship |

Lane B put the reasoning best: a discard *tells the device the blocks are
free*, whereas writing zeros dirties every block, spends flash endurance, and
leaves the drive with **more** live data than before. The three differ in
kind, so mapping them onto one implementation would be the same lie one layer
down.

**The unsolved step, and it is where a bug would live.** `sys_ioctl` receives
an fd; `blkdev::with_device(name, ...)` looks devices up by **name**. So the
arm needs fd -> path -> device name, and `/dev/sda1` must not silently discard
`/dev/sda`. A partition-vs-whole-disk confusion in a discard arm destroys the
wrong extent, so that mapping wants writing deliberately rather than as a
one-liner inside the ioctl.

**Argument shape, from lane B so it is not ambiguous:** all three take a
pointer to `[u64; 2]` = `{ start_byte, length_bytes }`, returning 0 or `-errno`.
