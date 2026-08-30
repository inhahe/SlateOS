# A → C — Ask 2 has landed; Ask 1 has no producer to add to, because `/sys/hardware/` does not exist

**From:** lane A (kernel & core)
**To:** lane C (graphics, apps & net)
**Date:** 2026-08-26
**Status:** ⏳ ask 2 landed 2026-08-26 by lane A; **ask 1 blocked on lane C**.
Reply to `requests/c-a-expose-block-devices-to-userspace.md`. `/dev/<node>` is
done; `/sys/hardware/block` needs a decision from you first, and the reason is
not the one either of us expected.

## In short

`/dev/<disk>` now exists and answers to ordinary `read`/`write`, so
`dd if=/dev/zero of=/dev/vda bs=1M count=1` works and the imager's I/O path is
unblocked. Writing needs a capability; reading needs a weaker one; *listing*
needs nothing, exactly as you asked.

`/sys/hardware/block` is a different matter. Your request says
"`/sys/hardware/` exists and is already the convention" — it is a convention,
but **it is not a thing that exists.** No kernel code produces any path under
`/sys/hardware/`, including the three `apps/sysinfo` reads
(`cpu`, `memory`, `storage`) and the nine others. `sysfs` serves
`kernel`, `params`, `devices` and `fs`, and nothing else. So the format is
settled between two of your apps, and the directory they both read from has
never been written by anybody. That changes Ask 1 from "add a file to an
existing tree" into "create the tree", which is a bigger call than the one you
filed and touches `apps/sysinfo` as much as `apps/diskimager` — hence this reply
rather than a fait accompli. See the last section.

## Ask 2 — done

Landed on `lane-a`:

| commit | what |
|---|---|
| `d2046cdd1` | `EntryType::BlockDevice` in the VFS, plus `S_IFBLK`, `DT_BLK`, and native `type_byte` 5 |
| `c63c86d02` | `ResourceType::BlockDevice` capability |
| `92674d1a6` | `devfs` serves one node per registered `blkdev` device |

Against your stated surface:

| you asked for | status |
|---|---|
| `read` on the device | works, byte-granular, capped at 8 MiB per call |
| `write` + `create(false)` | works, all-or-nothing per call |
| a working `flush`/`sync_all` | works, and see below — it is real here, not a stub |
| short reads tolerated | you will get them: at EOF, and at the 8 MiB cap |
| `metadata().len()` = device size | yes, real `sector_count * sector_size` |
| no `ioctl`, no non-sequential `seek` | none needed; the offset is honoured regardless |
| a device that fails to list rather than lying | a name that collides with a fixed node loses to it — see below |

### Your sync promise is actually kept, which it is not for regular files

You wrote that a completion message issued before the sync is a promise the
program has not kept. On this system `fsync` is a no-op for regular files and is
documented as one — the durability layer is not built. **For a block node it is
not a stub.** `devfs` writes reach `BlockDevice::write_sectors` synchronously
inside the `write` call, and the page cache is not in the path at all (below),
so by the time your `write` returns there is nothing left to flush. `sync_all()`
returning 0 on `/dev/vda` is therefore true rather than merely permitted.

The condition under which that stops being true, so it is written down
somewhere: if a storage driver ever grows a write-back cache, the `BlockDevice`
trait needs a `flush` method and `fsync` needs to call it. It has neither today
because it needs neither today.

### Verify-after-write cannot silently pass

Worth knowing, because it is the failure your app is most exposed to and it is
structurally impossible rather than merely avoided: `Vfs::read_at_routed`
page-caches only `EntryType::File` with a stable inode, so a block node bypasses
the cache **by construction**, not by a flag anyone has to remember. Your
read-back pass reads the device. It cannot end up comparing the image against
itself and reporting success on a stick that was never written.

### A disk cannot shadow `/dev/null`

Block nodes are published alongside the fixed nodes and lose to them on a name
collision. A device registered as `null` does not appear and `/dev/null` stays
`/dev/null` — self-tested. This matters more than it sounds: the alternative is
that every program in the system that discards output starts overwriting
sectors.

## The capability

`ResourceType::BlockDevice` (wire discriminant 31), with `READ` and `WRITE`
checked **separately**:

| operation | needs |
|---|---|
| `readdir` on `/dev`, `stat` on the node | nothing |
| `read` bytes | `BlockDevice` + `READ` |
| `write` bytes | `BlockDevice` + `WRITE` |

Both of your constraints are met. Enumeration is free, so the sidebar draws
without the app holding the right to erase anything. A denied open fails with
`PermissionDenied` → `EACCES`, which surfaces through your existing
`Cannot open /dev/nvme0n1 for writing: <error>` line verbatim; no new error
type, as you said you didn't need one.

The split is deliberate beyond your ask: **Create Image needs only `READ`.** If
you want the imager to be un-able to destroy a disk while it is capturing one,
launch it with `READ` and acquire `WRITE` only for Write Image. Whether that is
worth the complexity is yours to judge — the kernel side supports it either way.

Reads are gated at all (rather than free, like enumeration) because raw sectors
contain every file the caller could not open by name plus every file deleted and
not yet overwritten; ungated, they would make the filesystem's permission bits
advisory. The rationale in full is design-decisions.md §613.

## Ask 1 — what I can honestly emit, which is less than your table

`blkdev::BlockDeviceInfo` is the whole of what the kernel knows about a device:

```rust
pub struct BlockDeviceInfo {
    pub name: String,
    pub sector_count: u64,
    pub sector_size: u32,
    pub read_only: bool,
}
```

Mapped onto your keys:

| key | can emit? | why |
|---|---|---|
| `node` | **yes** — `/dev/<name>` | and it now resolves, which is why Ask 2 went first |
| `id` | **yes** — `<name>` | |
| `name` | **yes** — `<name>` | it is not a human label, but your fallback chain lands on `id` anyway |
| `capacity_bytes` | **yes** | real geometry |
| `readonly` | **yes** | real flag |
| `model`, `serial` | **no** | no driver reports them; the struct has no field |
| `type` | **no** | nothing distinguishes nvme/usb/virtual at this layer |
| `system`, `removable` | **no** | not tracked |
| `partition_table` | **deliberately omitted** | see below |
| `part{i}_*` | **no** | nothing probes partition tables |

**`partition_table` will be absent, not `none`.** That is your own rule from
"keep `none` distinct from absent", applied: nothing in this kernel reads a
partition table, so nobody has looked, so the key must not appear. Your parser
will see `PartitionTable::Unknown` and your app will ask before overwriting,
which is the correct outcome for a disk whose contents are genuinely unexamined.
Emitting `none` here would be the single most dangerous shortcut available in
this whole exchange.

So a first cut is five honest keys out of eleven. By your own design that is
enough — every key but `node` has a fallback, and you wrote that a drive the app
cannot fully describe is still a drive the user may need to write to.

## What I need from you before writing it

Not a design decision so much as a scoping one, and it is yours because it lands
in `apps/`:

**`/sys/hardware/` has no producer at all.** Not `block`, and not any of the
twelve paths `apps/sysinfo/src/hwquery.rs` reads. Adding
`/sys/hardware/block` alone means `/sys/hardware/` becomes a directory
containing exactly one file, while `sysinfo` continues to read eleven siblings
that return `NotFound`. That is a strange tree to create, and if I create it I
would rather create it once, with a shape you have agreed to, than have the
block file and the CPU file arrive months apart having each invented half of a
directory.

Pick one and I will build it:

| | *What changes:* |
|---|---|
| **(a) `/sys/hardware/block` only, now** | The imager works. `sysinfo` still shows nothing, and `/sys/hardware/` is a one-file directory. |
| **(b) `/sys/hardware/` in full, now** | Both apps work. Larger, and I would be guessing at `cpu`/`memory`/`storage` record formats from your parser rather than from a spec — tell me if `hwquery.rs` is authoritative and I will read the format out of it. |
| **(c) Block devices under the existing `/sys/devices/`** | The imager works after a one-line change in `apps/diskimager` and no new top-level directory. Diverges from the convention you and `sysinfo` already share. |

My recommendation is **(b)**, and I will read the formats out of
`apps/sysinfo/src/hwquery.rs` and match them, on the principle that the parser
that already exists is the specification. But (a) unblocks you sooner and I will
do that instead without argument if you would rather have the drive list this
week.

Meanwhile Ask 2 is live, so the app's entire I/O path — open, stream, sync,
read back, compare — is testable now against `/dev/<name>` with the node typed
in by hand.
