### A-SWAP-HAS-NO-SWAP-FILE-AND-CLAIMS-ANY-DISK-WITHOUT-AN-EXT4-SUPERBLOCK -- 2026-10-09 -- OPEN (lane A)

**Status:** OPEN (lane A). Found answering the operator's question on D-Q8
("Shouldn't we allow a swap file or at least a swap partition? What did we agree
on in the design files?", `operator-answers/2026-10-09-open-questions-answers.txt`).

**In short:** the design says SlateOS swaps to a **swap file** by default, with a
swap partition as an install-time choice (`design.txt`, the installer section:
"Use a swap file as default"; the settings list: "change size of swap file (if
we're not using a swap partition instead)"). The kernel has neither. It swaps
to compressed RAM, and -- at boot, unasked -- to the first 16 MiB of the first
virtio disk named `vdb` or `vda` that has no ext4 superblock at its start.
`swapon` refuses everything. And the boot-time claim is a data-loss hazard: a
disk with a partition table, a FAT or NTFS filesystem, or anything else that is
not a bare ext4 superblock is taken, and its first 16 MiB are overwritten the
first time memory runs short.

**What is there today.**

| Piece | Where | State |
|---|---|---|
| compressed RAM (zram), 256 slots | `mm::swap::init(256)`, `kernel/src/main.rs` step 9c | works |
| a disk area | `mm::swap::init_disk(dev, base_sector, slots)`, `kernel/src/mm/swap.rs` | works, but only as one contiguous run of sectors on a block device |
| choosing the disk | `kernel/src/main.rs` step 20e-2 | probes `vdb` then `vda`; skips a device only if `fs::ext4::probe` finds a superblock at byte 1024; takes 512 slots (16 MiB) from sector 0 |
| `swapon` / `swapoff` | `kernel/src/syscall/linux.rs` `sys_swapon`, `sys_swapoff` | `EPERM` after the flag check, by a comment that says "we have no swap subsystem" |
| a swap file | -- | nothing: no extent map, no signature, no pinning |
| `roadmap-detailed.md` §Swap | "[x] Swap file support" | wrong; corrected to `[ ]` the same day |

**How the hazard would bite.** `fs::ext4::probe` reads one superblock at the
start of the whole device. A partitioned disk keeps its ext4 inside a
partition, so the probe answers "no filesystem" and the disk is claimed: its
partition table and first partition are where the swap slots go. The same
happens to a FAT disk -- and step 20f then mounts FAT from `vda`, the device
20e-2 may just have taken. It needs a virtio disk (a virtual machine), and
memory pressure past what zram holds, which with 256 slots is 4 MiB.

**Proper fix** (taken by lane A, 2026-10-09):

1. **Never claim a device that does not say it is swap.** An area is used only
   if it carries a swap signature -- Linux's `mkswap` header (`SWAPSPACE2` in
   the last ten bytes of its first page, version 1, `last_page`), so Linux's
   tools make and read it -- or, at boot, if it is a GPT partition of the swap
   type (`0657FD6D-A4AB-43C4-84E5-0933C84B4F4F`, the Discoverable Partitions
   Specification's). A raw disk with neither is left alone. The boot test's
   `swap.img` gains a header.
2. **Areas of any shape.** A swap area becomes a list of sector extents rather
   than one run, so a partition is one extent and a file is the extents the
   filesystem gives it.
3. **A swap file**, the design's default: `swapon` on a regular file asks its
   filesystem (ext4) for the file's extents, refuses one with holes or
   not-yet-written extents, checks the signature, and marks the inode as an
   active swap file so writes and truncation are refused while it is in use
   (Linux's `S_SWAPFILE`). The kernel then reads and writes the sectors
   directly, as Linux does.
4. **`swapon` and `swapoff`** on both ABIs, with priorities and Linux's flags;
   `swapoff` brings every page back from the area before releasing it.
5. **Compressed RAM sized from the machine**, not a fixed 256 pages: the
   tier every page tries first holds 4 MiB today whatever the RAM.
6. **Who turns it on** is userspace's (lane B's boot sequence, from the
   configuration, as Linux's `swapon -a` reads `/etc/fstab`); the installer
   makes `/swapfile` by default and a partition if chosen; Settings changes the
   file's size (`design.txt`), lane C's.

**Where:** `kernel/src/mm/swap.rs` (`DiskBackend`, `init_disk`),
`kernel/src/main.rs` step 20e-2, `kernel/src/syscall/linux.rs`
(`sys_swapon`, `sys_swapoff`), `kernel/src/fs/ext4` (an extent query),
`scripts/boot-test.sh` (`swap.img`).
