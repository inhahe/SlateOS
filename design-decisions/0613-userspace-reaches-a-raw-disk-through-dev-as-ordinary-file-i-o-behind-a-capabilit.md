## 613. Userspace reaches a raw disk through `/dev`, as ordinary file I/O behind a capability — not through a bespoke syscall or a userspace driver

*Date: 2026-08-26*

**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** Two programs lane C is building — a disk imager (writes a
downloaded `.iso` onto a USB stick) and a partition editor — need to read and
write a whole *storage device*, not files stored on it. Nothing in the system
let a userspace program do that at all, so both were stuck. This decides *how*
they get to: the disk shows up as `/dev/vda` and you `read`/`write` it like a
file, except that moving bytes either way first requires holding a specific
permission token (a "capability" — an unforgeable object a program must be
*given*, as opposed to a right it has just for being root). Merely *listing*
which disks exist requires nothing.

`design.txt` does not settle this. It mandates a microkernel with drivers in
userspace and capability security from day one, both of which the chosen shape
honours, but it says nothing about whether a raw device is nameable in the
filesystem — so the fork was real and this entry records the call.

**The options.**

| | *What changes:* |
|---|---|
| **(a) Nothing** (status quo) | The imager and the partition editor cannot be written. Neither can `fsck`, `dd`, or a backup tool. |
| **(b) `/dev/<disk>` as ordinary file I/O, gated** (chosen) | `dd if=x.iso of=/dev/vda` works, if you hold the capability. Every existing tool that opens a path already knows how. |
| **(c) A bespoke syscall pair** (`SYS_BLOCK_READ`/`SYS_BLOCK_WRITE`) | Same power, reached by a name no ported program knows. Every tool needs porting; the ABI grows two entries that duplicate `read`/`write`. |
| **(d) Route through a userspace storage daemon over IPC** | Same power, one process hop further away. The daemon does not exist, and would need this same kernel interface underneath it to do its job. |

**Why (b).** The decisive argument is not convenience, it is that the
alternatives do not actually remove the kernel interface — they wrap it. (d) is
the microkernel-purist answer, and it is the right *eventual* shape for
scheduling and arbitration, but a storage daemon has to reach the sectors
somehow, and the way it would reach them is (b). Building (d) first means
building (b) anyway and then hiding it, which is a larger change that leaves
the same attack surface. (c) is (b) with the names changed and every ported
program broken.

The real question behind the fork is whether *naming* a device in the
filesystem is what grants access to it — the mistake Unix makes, where holding
root gets you `/dev/sda` by ambient authority. It is not, here: the name is
public and the bytes are not. `readdir` and `stat` answer freely; `read` and
`write` demand `ResourceType::BlockDevice` with `READ` or `WRITE`
respectively. So a settings panel can draw a list of disks without being
launched with the authority to erase them, which is the property (a)-through-(d)
were really being judged on.

**Why reads are gated and not just writes.** Writing a raw disk is the most
destructive thing a userspace program here can do, so gating it needs no
argument. Gating reads does: raw sectors contain every file the caller could
not open through the ordinary path, plus every file that was deleted and not
yet overwritten. Ungated reads would make the filesystem's permission bits
advisory. Keeping the two rights separate is what lets a backup tool hold the
authority to capture an image without holding the authority to destroy one.

**The safety property that made this the *easy* call.** `Vfs::read_at_routed`
page-caches only `EntryType::File` with a stable inode. A block node is
therefore uncached *by construction*, not by a flag someone must remember to
set. That is load-bearing for the imager's verify-after-write pass: with a
cache in the path it would compare the image against itself and report success
on a stick that was never written. An interface that made verification lie
would have been a reason to prefer (c) or (d); this one cannot.

**What this does not decide.** Partition-table parsing, device model/serial
strings, and hot-plug notification are all absent — `blkdev::BlockDeviceInfo`
carries only name, sector count, sector size and a read-only flag, and
`/sys/hardware/block` will report only what is actually known rather than
inventing plausible values. See the reply in
`requests/a-c-block-devices-are-served-but-sys-hardware-has-no-producer.md`.

**Where it bites.** `kernel/src/fs/devfs.rs` (`find_block`, `block_read_at`,
`block_write_at`, `require_block_cap`); `kernel/src/cap/mod.rs`
(`ResourceType::BlockDevice`); `kernel/src/fs/vfs.rs`
(`EntryType::BlockDevice`). Requested in
`requests/c-a-expose-block-devices-to-userspace.md`.
