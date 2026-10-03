## §664 — a volume label is metadata, not a directory entry, and the VFS is where that is guaranteed

**Date:** 2026-09-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** A FAT-formatted disk has a name — "MYDISK" — and FAT stores that
name in a slot in the disk's top-level folder, alongside the actual files. That
is a storage trick, not a claim that the disk contains a file called MYDISK;
copying the disk should not produce a file by that name. It never did, but only
because the FAT driver remembered to hide the slot in each of the three places
it could have shown it. Now the layer above guarantees it instead, so the next
driver with a disk name in its top-level folder cannot reintroduce the problem
by forgetting.

### The question lane B asked, and why the answer was "no, but"

Lane B, wiring `SYS_FS_GETDENTS_PINNED` (664), noticed that `SYS_FS_LIST_DIR`
(603) skips `EntryType::VolumeLabel` while `SYS_FS_READDIR_AT` (647) and 664
both emit it as type byte 2, and asked whether one directory therefore had two
contents depending on which call asked — with a concrete consequence: a `cp -r`
of a FAT volume through 664 acquiring a spurious entry named after the label.
B offered to filter type 2 client-side and argued, correctly, that the client
is the wrong place: every client has to remember, and the one that forgets
fails silently.

It does not happen. `FatFs` is the only producer of the variant, and it filters
at all three routes into `to_vfs_entry` — `readdir` (`fat.rs:3127`),
`readdir_at` (`:3163`), and `resolve_path` (`:1888`), the last of which is what
makes it airtight, since the other two `to_vfs_entry` call sites go through it.
No `DirEntry` carrying the variant has ever left the driver. 603's `continue`
and 647/664's `=> 2` are all unreachable; type byte 2 has never been emitted.

**So the answer to B was "there is no divergence" — and that answer is worth
less than it sounds.** Three syscalls agreed about the contents of a directory
because one driver's author filtered in three separate places, and nothing
anywhere recorded that they were relying on it.

### The decision

**Filter at the VFS**, in one helper (`Vfs::drop_volume_labels`) called from
`Vfs::readdir`, `Vfs::readdir_at_resolved` and `Vfs::readdir_pinned`.

*The case against* is real and is why this was not obviously right: it is a
`retain` over every listing on a path the design spec calls performance-
critical, to remove a value that cannot occur, in a kernel whose own rules say
not to add abstraction that buys nothing measurable. Defensive code against an
impossible input is usually just cost.

*The case for*, which won:

- **The input is not impossible, it is merely absent today.** exFAT has a
  volume label in its root directory and is a plausible next driver. Whoever
  writes it has no reason to know that three syscall handlers depend on their
  remembering to filter. The invariant's lifetime is currently bounded by one
  future author's diligence.
- **The failure is silent and untestable in the ordinary way.** A test cannot
  distinguish "the label was filtered" from "there was no label" without
  independently proving the label exists — which is the same trap that hid four
  disagreeing `mkdir` masks for two days (§663), found the same week, one file
  away.
- **The cost is a `retain` on an already-allocated `Vec`, downstream of block
  I/O and an allocation.** It does not appear against the noise of either.
- **The guarantee belongs to the layer that states it.** 664's whole purpose is
  to be the race-free substitute for a listing taken by path; "these two calls
  list the same thing" is a VFS-level promise, so the VFS should be what keeps
  it, not three drivers independently.

Drivers keep their own filters — FAT's are cheaper there and its short-name
collision checks need them — but they stop being load-bearing.

**Placement was the only genuinely delicate part.** The filter runs *before*
`readdir_at_resolved` takes `total` and slices the page, and *before* 664 folds
`needed`. 647's old comment argued at length that a label must consume a record
because dropping one mid-pack would make `entries_written` disagree with how
far the offset advanced, so the caller's next page would step over a real
neighbour. That argument is correct, and it was always an argument about
*where* to filter, never about whether — which is exactly why it is honoured by
moving the filter up rather than by passing labels through.

### What the ABI says now

Entry-type byte `2` is **reserved and never emitted** on 603, 647 and 664.
Reserved rather than reclaimed: renumbering 3/4/5 down would break every
decoder to recover one value. The label is read from `SYS_FS_STATVFS` (608)
and written by `Vfs::set_volume_label`, neither of which goes through a
listing. 664's ABI note now states outright that it lists exactly what 603 and
647 list, because that is the property a caller swapping routes to close a race
is actually relying on, and it should not have to infer it from three handlers
happening to match.

### The test, and the order its assertions are in

`fat::mkfs_self_test` already formatted a RAM disk with the label `SELFTEST`,
so the fixture existed. It now asserts, in this order:

1. `Vfs::statvfs(mp).volume_label == "SELFTEST"`;
2. no `VolumeLabel` entry and no entry named `SELFTEST` from `readdir`,
   `readdir_at` or `readdir_pinned`;
3. the three routes report the same set of names.

**Assertion 1 is not a warm-up.** Without it, 2 passes on a volume that has no
label at all, and the test proves nothing — the identical mistake to asserting
a directory mode of `0o755` against a mask that clears `0o1000`. Assertion 3
states B's actual requirement directly instead of leaving it to be inferred.

### The bug this actually found, which was not the one asked about

Reading both listings end-to-end in order to answer B turned up that
**`Vfs::readdir_pinned` never injected submounts.** `Vfs::readdir` and
`Vfs::readdir_at_resolved` both add mounted filesystems' mount points to a
listing — `/mnt/usb` appears when you list `/mnt` even though the underlying
filesystem has no such directory — and `readdir_pinned` did not. So 664 listed
only what the backing filesystem physically contains: no `tmp`, `proc`, `sys`
or `dev` at the root. A `cp -r` that switched to 664 precisely to stop a rename
racing its walk would silently have stopped descending into every mounted
filesystem, and reported success.

That is B's question in the opposite direction, in the same pair of calls —
and unlike the volume label it was **real**. It sharpens this entry's own
justification uncomfortably: the filter above was argued for against a
*hypothetical* future driver that forgets a rule, while the identical failure
mode was concurrently *true*, in the same three functions, about a different
rule. "Three sites that happen to agree" was not a risk here; it was already a
bug.

So it was fixed in the same change, and generalised: **`Vfs::finish_listing`**
is now the single route by which a raw driver listing becomes a VFS listing —
it drops labels *and* injects submounts — and all three entry points call it
instead of open-coding zero, one or both steps. One function, not three sites
that must match. `readdir_pinned` scopes its backing-filesystem guard so it
drops before `finish_listing` takes `VFS`, preserving the lock order (`VFS` is
never taken while a filesystem lock is held).

**And the test nearly missed it.** The three-route agreement check described
above *passed with the bug present*: the FAT volume's root has no submounts,
and three routes that all omit the same thing agree perfectly. It can only see
this because it now mounts a memfs inside the volume first, at a mount point
with no physical directory behind it, so the entry can only come from
injection. That is §663's lesson in a second costume — an agreement test proves
nothing about a case none of the parties encounters, just as a mask can only be
tested by a value outside it.

### Still open

`FileSystem::readdir_at` has a default implementation, a doc comment urging
implementors to override it for efficiency, overrides in FAT and ext4 — and no
callers at all, because `readdir_at_resolved` open-codes the default in order
to dedup submount names against the whole listing. So 647 on a large directory
reads and formats every entry to return one page, and ext4's native paginated
walk has never run. Logged as
`A-READDIR-AT-TRAIT-METHOD-HAS-TWO-IMPLEMENTATIONS-AND-NO-CALLERS` in
`known-issues.md`; the real fix needs a mount-time policy on mount-point names
colliding with real directory entries, which is a decision, not a patch.

Neither was on anyone's list. Both turned up because answering "is there a
divergence between these two listings?" required reading both listings' full
path from syscall to driver, which is the third time in two days that tracing
one value end-to-end has found something nobody was looking for.
