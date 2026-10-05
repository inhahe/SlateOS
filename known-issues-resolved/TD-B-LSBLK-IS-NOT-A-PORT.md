## TD-B-LSBLK-IS-NOT-A-PORT (lane B, 2026-09-27) — ✅ FIXED 2026-09-27 (lane B)

**Fixed:** `userspace/lsblk` is now util-linux 2.39.3's `lsblk.c`,
`lsblk-devtree.c`, `lsblk-mnt.c` and `lsblk-properties.c`, ported function by
function onto `smartcols`, `ulmount`, `ulblkid` and `ulsysfs`.
`scripts/lsblk-diff.sh` compares it with WSL's `lsblk from util-linux
2.39.3` in C.UTF-8 and C: util-linux's two `--sysroot` snapshots (an LVM and
an NVMe machine) with every `.cols` file and 68 option sets each (every
format, sort, dedup, tree column, width, filter, `--merge`, `--inverse`,
every column group), the live machine as an ordinary user (udev's database,
mounts, swap, named devices), and option refusals: **502 agree, 0 differ**.
One judgment call: udev is asked only where it runs (`/run/udev/data`), so
on SlateOS, which has none, libblkid is (todo.txt, lane B Judgment Calls,
2026-09-27). What remains unmeasured is root's libblkid path -- under WSL udev
always answers first, as it does upstream.

The history, kept:

**In short:** `lsblk` (list block devices) is a hand-written program, not a
port of util-linux's. It recognises filesystems by its own code where
util-linux's asks libblkid, which is now ported (`userspace/ulblkid`,
measured against the real libblkid on 621 images). `wipefs` was in the same
state and is now a port (2026-09-27, `scripts/wipefs-diff.sh`: 239 cases
agree, the erased images' bytes and the backups included).

**What a user sees:** `lsblk -f` shows no FSTYPE, LABEL or UUID for the
formats its own code does not know; options are parsed by hand (whole long
names only), and its output formats approximate upstream's.

**Where:** `userspace/lsblk/src/main.rs`.

**The proper fix:** port `misc-utils/lsblk*.c` onto `ulblkid`, `ulmount`
and `smartcols`, with a differential harness against WSL's.

**Progress (2026-09-27):** the two libsmartcols features lsblk depends on
and the port lacked are in: sorting (lsblk sorts every table, by MAJ:MIN
unless `--sort` says otherwise, and `--list --raw/--pairs/--inverse` by tree
too) and line groups (`--merge`'s chart). `scripts/smartcols-diff.sh`
compares the port with util-linux's own libsmartcols.so.1 on 1500 generated
tables in two locales: 2954 agree, 764 of them drawing a group chart, 22
aborting on both sides where upstream aborts; the 46 upstream never finishes
(the known narrow-terminal loop, and a recursion through a line made its
own group's child) the port finishes. What remains is lsblk itself and the
`lib/sysfs.c` helpers it needs that `ulsysfs` lacks (the device chain and
subsystems, hot-plug, the SCSI host/attribute tests and HCTL).

**Also:** the hand-written `wipefs` answered to `blkdiscard` too -- a
personality no executable was ever produced for (the multicall baseline
listed it as unreachable). `blkdiscard` is now a port of util-linux's in a
crate of its own (`scripts/blkdiscard-diff.sh`: 23 cases, all an ordinary
user can reach).
