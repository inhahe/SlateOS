## TD-B-ULMOUNT-PROBES-ONLY-EXT (lane B, 2026-09-26) — ✅ FIXED 2026-09-27 (lane B)

**In short:** to learn a disk's filesystem type, label or UUID, util-linux
reads the disk's first blocks and compares them against over a hundred
known layouts ("probing", libblkid's job). The port in `userspace/ulmount`
knows five of them -- ext2, ext3, ext4 and their test and journal
variants -- and no partition tables. So for a device holding anything else
(a FAT EFI partition, a swap area, XFS, btrfs), or for a partition's own
PARTUUID and PARTLABEL, probing finds nothing where upstream names it.

**What a user sees:** where udev is running, nothing -- `findmnt` asks
udev's database first, as upstream does, and udev did the probing. Where it
is not (SlateOS today, and any root that cannot read `/run/udev/data`):

- `findmnt -o UUID,LABEL,PARTUUID` shows blanks for non-ext filesystems and
  for every PARTUUID/PARTLABEL.
- `findmnt --verify` says `cannot detect on-disk filesystem type (reason
  unknown)` for a vfat, swap, xfs or btrfs entry that upstream checks, and
  so cannot say `vfat does not match with on-disk ...` either.
- libblkid's cache, as root writes it (`/run/blkid/blkid.tab`), lists only
  ext devices' tags, so `findmnt -o SOURCES` and a `LABEL=` source are only
  resolved for ext filesystems when their udev link is missing.

For ext filesystems the port reports exactly libblkid's values (LABEL,
UUID, EXT_JOURNAL, SEC_TYPE, BLOCK_SIZE, LOGUUID, TYPE, in its order, with
its checksum check and its rule that two probers claiming one superblock is
no answer), and an unreadable device fails with `EACCES` as upstream's does.

**Where:** `userspace/ulmount/src/blkid.rs` -- `probe_file` and
`probe_ext`; everything above them (`mnt_cache_read_tags`, `mnt_get_fstype`,
`blkid_verify`) is ported whole and calls these two.

**How to see it:** on SlateOS, `findmnt -o TARGET,UUID` with the EFI
partition mounted; or `findmnt -x -F` an fstab with a swap line whose
device is readable.

**The proper fix:** port libblkid's probing as its own crate -- `probe.c`
(the chains, `blkid_do_safeprobe`'s ambivalence and "tolerant" rules, the
tiny-device and CD-ROM cases), the superblocks chain in its list order
(`superblocks/*.c`; vfat, swap and iso9660 first, since those are what
SlateOS images and its users' disks carry, then the rest) and the
partitions chain (`partitions/*.c`: dos and gpt first, for PARTUUID and
PARTLABEL) -- and have `ulmount::blkid::probe_file` call it. `blkid`,
`lsblk`, `wipefs` and `findfs` need the same crate, so it is one job for
all of them; each prober gets the differential treatment the programs got,
on disk images built by `mkfs.*` in WSL.

**Fixed (2026-09-27):** libblkid's probing is ported whole as its own
crate, `userspace/ulblkid` -- `probe.c`, all 79 superblock probers, all 13
partition-table probers (nested BSD, Minix, Solaris and UnixWare tables
included) and the topology chain -- together with libblkid's device cache
and tag evaluation, which moved there from `ulmount`. `ulmount` now probes
through it, so every filesystem, RAID member and partition table libblkid
knows is recognised by `findmnt` (tags, `--verify`) where only the ext
family was. `blkid` and `findfs` are ports of util-linux's programs on top
of it (`findfs` a crate of its own, as upstream's is a program of its own).

Measured: `scripts/blkid-diff.sh` compares every value, byte for byte,
with WSL's libblkid 2.39.3 -- safeprobe (and again accepting bad
checksums), fullprobe, the wipefs walk, the binary partition list with and
without FORCE_GPT -- and 621 images agree, none differs: util-linux's 128
test images, 9 made with mkfs/mkswap/sfdisk, 484 truncated copies.
`scripts/blkid-cli-diff.sh` compares the two programs: 144 cases agree.

What remains for other programs is theirs: `lsblk` and `wipefs` are still
hand-written and did not use the crate -- `wipefs` is now a port; `lsblk` is TD-B-LSBLK-IS-NOT-A-PORT.
