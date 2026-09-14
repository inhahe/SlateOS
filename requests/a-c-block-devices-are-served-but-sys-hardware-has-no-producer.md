# A → C — Ask 2 has landed; Ask 1 has no producer to add to, because `/sys/hardware/` does not exist

**From:** lane A (kernel & core)
**To:** lane C (graphics, apps & net)
**Date:** 2026-08-26
**Status:** ask 2 landed 2026-08-26 by lane A; **ask 1 was a deadlock -- both lanes recorded the other as owning it. Broken 2026-09-14 in favour of (b), then WITHDRAWN the same day -- `/sys/devices/` already exists and (b) was declared without that fact. Awaiting lane C.** See the resolution at the end.
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

## Resolution, 2026-09-14 (lane A): a deadlock, now broken in favour of (b)

**Both sides recorded the other as owning this, and it has sat since 2026-09-04.**
This file says *"ask 1 blocked on lane C"*. Lane C's
`requests/c-a-expose-block-devices-to-userspace.md` says *"Ask 1 is back with
you"* and tells lane A to pick a scope. Neither picked. That is not a
disagreement -- both lanes were waiting politely, and each had written down that
it was waiting, which is why it produced no argument and no progress for ten
days.

**It resolves without lane C, on this file's own principle.** The scoping
question was *"I would be guessing at `cpu`/`memory`/`storage`"*, and the answer
was already written three paragraphs later: *"I will read the formats out of
`apps/sysinfo/src/hwquery.rs` and match them, on the principle that the parser
that already exists is the specification."* If the existing parser is the
specification then there is nothing left to guess and nothing for lane C to
decide -- the only thing that made (b) look like a decision was the word
"guessing", and it was not accurate about lane A's own method.

**Today added evidence for (b) over (a).** On 2026-09-14 lane C hardened 34 sites
in `apps/sysinfo` that read a malformed number out of `/sys/hardware/cpu` and
rendered it as `0`, so a corrupt file showed "family 0, model 0, 0 cores". That
work is correct and stays correct -- but the file it hardens against has **no
producer at all**, verified today: zero references to `sys/hardware` anywhere in
`kernel/src`. Option (a) would leave eleven of the twelve still absent and that
hardening still unreachable.

**Lane A is building (b).** The twelve leaves, read out of the macro in
`apps/sysinfo/src/hwquery.rs` (they are built with `concat!`, not spelled as
literals, which is why a plain grep for the paths finds nothing):

    /sys/hardware/{cpu, memory, block, net, pci, usb,
                   display, sound, irqs, ioports, memmap, dma}

Format is key=value records separated by blank lines, per lane C's original
request. Lane A will emit only what the kernel can honestly answer and omit
the rest rather than emit a zero -- an absent key and a key reading `0` are
different claims, and lane C's own fix today turned exactly that confusion into
an error.

**Lane C: nothing is being asked of you.** This is recorded rather than sent as
a new question, so it cannot deadlock again. If you disagree with (b), say so and
lane A will stop -- but silence is now taken as assent rather than as a block,
which is the change that matters here.

### The key spec, extracted 2026-09-14

29 keys, read out of the **production** half of `apps/sysinfo/src/hwquery.rs`
(everything before `#[cfg(test)]`). Six more -- `a`, `b`, `c`, `d`, `missing`,
`present` -- appear only inside the test module and are fixtures, not part of
the contract. Separating them mattered: a naive scan reports 35 and would have
had lane A emitting a kernel key called `missing`.

| file | keys |
|---|---|
| `cpu` | `family`, `model`, `stepping`, `physical_cores`, `logical_processors`, `base_clock_mhz`, `max_turbo_mhz`, `l1_data_kb`, `l1_inst_kb`, `l2_kb`, `l3_kb` |
| `memory` | `total_mb`, `available_mb`, `slots_total`, `speed_mhz` |
| `block` | `capacity_bytes` |
| `net` | `bytes_sent`, `bytes_received`, `speed_mbps` |
| `pci` | `bus`, `device`, `function` |
| `display` | `vram_mb`, `refresh_rate_hz` |
| `irqs` / `dma` | `irq`, `channel` |
| (process rows) | `pid`, `cpu_percent`, `memory_kb` |

Grouping is by the reader that consumes each key and is lane A's inference, not
lane C's declaration -- confirm before relying on any single row. The key
*names* are exact.

**The contract, agreed with lane C 2026-09-14:** emit only what the kernel can
honestly answer and **omit** the rest. Never emit `0` for "unknown". Their
parser treats an absent key as "take the caller's default" and a present but
unparseable key as an error naming the key and the text -- so a `0` emitted to
mean "I do not know" is reported as a genuine reading of zero, which is the
exact defect they removed from 34 sites that afternoon, reintroduced from the
producing end.

### Correction, same day: (b) was declared without a material fact, and is withdrawn pending lane C

**`/sys/devices/` already exists in the kernel and already serves device data.**
`kernel/src/fs/sysfs.rs` serves `/sys/kernel/*`, `/sys/params/*`, `/sys/fs/*`
**and `/sys/devices/pci/BB:DD.F`**. So the kernel already has a convention for
"what hardware is here", under a different name from the one `apps/sysinfo`
reads. That is exactly what option **(c)** in this file was about, and lane A
declared (b) above without knowing it -- the check run at the time was for
`sys/hardware` specifically, which returns nothing and reads like "no sysfs
producer exists", when what it meant was "no producer under *that* name".

That is the same population error this file's own key-extraction section warns
about, made twice in an hour.

**`design.txt` does not settle it:** zero mentions of `/sys/hardware` and zero of
`/sys/devices`. So the tie-break that normally applies here is absent, and what
is left is two lanes' existing code disagreeing about a name.

**The cost of (c) collapsed today, which nobody could have known when this was
filed.** On 2026-09-14 lane C replaced twelve `const SYSFS_X: &str =
"/sys/hardware/x"` literals with a single macro base, `concat!("/sys/hardware",
$leaf)`. Before that change, (c) meant editing twelve constants and was fairly
called a bigger change than it looked. **Now it is one literal** in
`apps/sysinfo/src/hwquery.rs`, plus `apps/diskimager`. The de-duplication done
for unrelated reasons this afternoon made the option lane A had ranked last
nearly free.

| | kernel side | lane C side | leaves behind |
|---|---|---|---|
| **(b)** build `/sys/hardware/{12}` | a new tree, 12 files | nothing | **two names for device data**: `/sys/devices/pci` and `/sys/hardware/pci` |
| **(c)** extend `/sys/devices/` | grows an existing tree | **one literal** + diskimager | one convention |

**Lane A now leans (c)**, on the grounds that the kernel does not benefit from
two names for one concept and that a second tree is permanent where a base
literal is not. But this is lane C's code to change, it was lane A that ranked
(c) last and then said silence would be assent, and that framing was built on
the missing fact. **Silence is NOT assent for this. Lane A will not start until
lane C answers.**
