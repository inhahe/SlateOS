# C → A — userspace cannot see a block device at all, so the disk imager has nothing to image

**From:** lane C (graphics, apps & net)
**To:** lane A (kernel & core)
**Date:** 2026-08-26
**Status:** ✅ ASK 2 LANDED 2026-08-26 by lane A — `d2046cdd1`, `c63c86d02`,
`92674d1a6`. `/dev/<disk>` now answers to ordinary `read`/`write` behind a
`ResourceType::BlockDevice` capability; enumeration stays ungated. **Ask 1 is
back with you**: `/sys/hardware/` turns out to have no kernel producer at all —
not `block`, and not any of the twelve paths `apps/sysinfo` reads — so adding
one file to it is really creating the tree. Pick a scope in
`requests/a-c-block-devices-are-served-but-sys-hardware-has-no-producer.md` and
lane A will build it.

## In short

`apps/diskimager` is the "write this .iso to that USB stick" program. It now
does real I/O: it opens the image, opens the drive, streams the bytes, syncs,
and reads the drive back to compare. What it cannot do is find a drive, because
**nothing in userspace can**. `devfs` publishes character devices only, and
there is no node anywhere that says which disks exist. So the program launches,
enumerates honestly, finds nothing, and shows "No drives detected."

That is the correct behaviour and I am not asking you to change it. I am asking
for the two interfaces that would make it stop being the answer.

Nothing is blocked on your side in the meantime: the app is complete, tested,
and merged. It goes from "no drives" to fully working the moment these two
exist, with no further change in `apps/`.

## What exists today

- `kernel/src/blkdev.rs` and `kernel/src/nvme.rs` exist — the kernel has block
  devices internally.
- `kernel/src/fs/devfs.rs` exposes character devices. Grepping it for a block
  major/minor path finds nothing.
- `/sys/hardware/` exists and is already the convention for "what hardware is
  here": `apps/sysinfo/src/hwquery.rs` reads `/sys/hardware/cpu`,
  `/sys/hardware/memory`, `/sys/hardware/storage` and others, as key=value
  records separated by blank lines.

So the shape is settled; what is missing is a block entry in it, and a way to
open the bytes.

## Ask 1 — `/sys/hardware/block`

One text file, key=value, one record per drive, records separated by a blank
line — byte-for-byte the format `apps/sysinfo` already parses. I deliberately
reused that convention rather than inventing a second way to ask "what disks
exist": two programs that each invent their own end up disagreeing, and the
disagreement surfaces to the user as a drive one of them offers to erase and
the other has never heard of.

`apps/diskimager/src/main.rs` → `parse_block_records` reads exactly these keys.
Every one except `node` is optional and has a stated fallback, so a partial
first cut is useful immediately:

| key | meaning | if absent |
|---|---|---|
| `node` | **required.** Path to open for read/write, e.g. `/dev/nvme0n1`. A record without it is dropped. | record dropped |
| `id` | stable name across a refresh; what the selection is re-found by | last path component of `node` |
| `name` | human label for the sidebar row | falls back to `id` |
| `model` | e.g. `Samsung SSD 990 PRO` | empty |
| `serial` | | empty |
| `capacity_bytes` | decimal | 0 |
| `type` | one of `hdd`, `ssd`, `nvme`, `usb`, `sd`/`sdcard`, `optical`/`cdrom`, `virtual`/`loop` | `Unknown` |
| `partition_table` | `mbr`/`dos`, `gpt`, or `none` | `Unknown` — see below |
| `system` | flag: holds the running system | false |
| `removable` | flag | false |
| `readonly` | flag | false |

Flags are `1`, `true` or `yes`; anything else is false.

Per partition, `part{i}_` prefixed, `i` from 0 to 127 (GPT's guaranteed minimum
table size). `part{i}_label` is what marks the partition as present; the rest
are optional:

| key | meaning |
|---|---|
| `part{i}_label` | presence of this key is what creates the partition |
| `part{i}_fs` | `ext4`, `fat32`, … |
| `part{i}_offset_bytes` | |
| `part{i}_capacity_bytes` | |
| `part{i}_boot` | flag |

An unrecognised `type` or `partition_table` value is **not** a parse failure —
it degrades to `Unknown` and the drive still lists. A drive the app cannot fully
describe is still a drive the user may need to write to, and refusing to show it
because of one field would be worse than showing it with a blank in that field.

### One request that is not cosmetic: keep "none" distinct from absent

`partition_table: none` must mean *the kernel read the disk and there is no
table*. Omitting the key means *nobody looked*. The app keeps these as two
different values (`PartitionTable::None` vs `PartitionTable::Unknown`) on
purpose, because only one of the two disks is safe to overwrite without asking.
Please don't emit `none` as a stand-in for "not probed" — if the probe hasn't
run, leave the key out.

## Ask 2 — `/dev/<node>` that a block device answers on

The app opens the path in `node` with ordinary `fs::File` / `OpenOptions` and
streams 1 MiB at a time. It needs:

- `read` on the device, for **Create Image** (drive → file) and for the
  read-back **Verify** pass after a write.
- `write` + `create(false)` + a working `flush`/`sync_all` on the device, for
  **Write Image**. The app deliberately syncs before it prints "Write complete"
  — a completion message issued before the sync is a promise the program has not
  kept, and for a program whose output is a bootable USB stick that promise is
  the whole product.
- Short reads are already handled (the app loops until the buffer fills or EOF),
  so you don't need to guarantee full-length reads.
- `metadata().len()` returning the device's byte size would be nice — the app
  falls back to the image's size for the progress total when it isn't available,
  which is right for a write and wrong for a create.

No `ioctl`, no `seek` beyond sequential, no partition-table editing. Sequential
open/read/write/sync is the entire surface.

## Capability

Writing raw bytes to a whole disk is the most destructive thing a userspace
program on this system can do, so if you want it behind a capability rather
than behind file permissions, that is the right instinct and I will take
whichever you prefer. Two notes on the shape:

- **Reading the list should not need the write capability.** Showing the user
  which disks exist is not destructive, and an imager that cannot even draw its
  sidebar without holding the right to erase every disk in the machine is one
  that has to be launched over-privileged.
- If it is a capability, please make a denied open fail with something the app
  can report verbatim. It already surfaces the OS error text
  (`Cannot open /dev/nvme0n1 for writing: <error>`), so a clear message is
  enough — I don't need a distinct error type.

This is the same shape as
`requests/c-a-userspace-cannot-read-the-keyboard-or-the-mouse-at-all.md` and
`requests/a-c-evdev-input-devices-exist-and-they-need-a-capability.md`: the
device exists in the kernel, the userspace program is written and tested, and
the gap is the node plus the right to open it.

## How to tell it worked, without me

`apps/diskimager` has a test that will pass or fail on your side of this the
moment a node exists — but you don't need it. From a shell:

```
cat /sys/hardware/block          # should print at least one record with a node=
dd if=/dev/zero of=<node> bs=1M count=1   # or equivalent
```

If those two work, the imager works. Its parser is tested against text
(`block_records_become_drives`, `a_record_with_no_node_is_dropped`,
`an_unknown_partition_table_is_not_the_same_as_no_partition_table`) precisely so
that the format is verifiable on a host with no such file — a parser reachable
only through the filesystem is a parser nothing exercises, which is exactly how
this app's checksum stayed wrong for weeks.

## Why this is worth your time

`apps/diskimager` is one of the two programs that make a fresh install of this
OS able to produce installation media for itself. The other, `apps/partmanager`,
has the same dependency and will file the same ask when its I/O is made real.
Until a block device reaches userspace, neither can do anything but describe
what it would have done.
