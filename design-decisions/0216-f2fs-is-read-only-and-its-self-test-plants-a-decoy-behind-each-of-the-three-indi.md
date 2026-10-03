## §216 — F2FS is read-only, and its self-test plants a decoy behind each of the three indirections a reader must resolve

**Date:** 2026-08-16
**Decided by:** Claude (autonomous)
**Lane:** A
**Where:** `kernel/src/fs/f2fs/` (`raw.rs`, `sb.rs`, `cp.rs`, `node.rs`,
`dir.rs`, `mod.rs`, `tests.rs`), `kernel/src/fs/mod.rs`,
`kernel/src/main.rs` (self-test call site), `kernel/src/kshell.rs` and
`kernel/src/syscall/handlers.rs` (mount dispatch)

**In short:** F2FS is the filesystem Android puts on virtually every phone's
data partition, and a common choice for SD cards and consumer SSDs, so an SD
card pulled out of a phone is unreadable to us without a driver. This adds one
that can *read* an F2FS volume but refuses every write. Reading F2FS is unlike
reading ext4 in one specific way that drove every decision here: **a file's
data block is not named by the inode directly.** To get from a path to bytes
you must first pick the right checkpoint out of two, then use it to pick the
right node-address table out of two, then check whether that same checkpoint
carries a *third* answer in a journal that overrides both. Each of those three
steps has a wrong answer that is a perfectly valid block full of plausible
data. A driver that gets one wrong does not crash — it hands back somebody
else's bytes. Three calls follow from that: read-only rather than read-write;
a fake disk built in memory rather than a test that only runs when someone
plugs in a real phone's SD card; and, in that fake disk, deliberately parking
a *valid but wrong* answer at every one of the three wrong turns, so a bug
fails a named check instead of quietly succeeding.

### Decision 1 — read-only, and the mount says so

F2FS is a log-structured filesystem: it does not overwrite a data block in
place, it appends a new one to the tail of an open segment and then rewrites
the node that pointed at the old one. That is the whole point of the design —
it is what makes it kind to flash, which erases in large blocks and hates
in-place rewrites — but it means a single one-byte write touches, at minimum,
the data segment, the node containing the pointer, the NAT entry naming that
node's new location, the SIT (segment information table) bitmap marking the
old block dead and the new one live, the SSA (segment summary area) recording
which node owns the new block, the free-space accounting, and finally a new
checkpoint pack to make all of it visible atomically. Get the SIT bitmaps or
the free-block counts wrong and the garbage collector — which *moves live data
around by consulting exactly those structures* — will later relocate or
overwrite blocks that are still referenced. The damage does not appear at
write time; it appears the next time the volume is mounted by a real F2FS
implementation, with a file that has silently acquired someone else's
contents.

**Alternative considered:** implement writes now. Rejected for the same
reason as Btrfs (§215) and NTFS: a read driver that is wrong returns bad data
to *us*, and we find out immediately because the self-test asserts on it; a
write driver that is wrong corrupts *the user's* volume, and they find out
later, on a different operating system, with no way to attribute it. The
asymmetry is not close. Read support is also the part that has a user today —
mounting a phone's SD card, recovering files off a device that will not boot —
whereas write support only matters once F2FS is a filesystem we would *install
onto*, which it is not: ext4 is (per `design.txt`) the filesystem we install
onto, and the roadmap entry for F2FS sits under "additional filesystems."

Consequently `F2fsFs` reports `read_only: true` from `statvfs`, masks every
mode it returns down to `0o555`/`0o444`, and implements only the reading half
of the `FileSystem` trait. Writes do not fail deep in the driver with a
confusing error; they are not reachable.

There is one refusal worth calling out separately: **`FEATURE_BLKZONED` is
rejected at mount** rather than ignored. A zoned volume's main area is laid
out against the drive's zone boundaries, and a reader that ignores the flag
computes plausible-looking block addresses that are simply wrong. Refusing to
mount is the honest outcome; returning wrong bytes is not.

### Decision 2 — the self-test builds its own volume in RAM, so it runs on every boot

The obvious way to test a filesystem driver is against a real image. The
problem is that nobody plugs an F2FS-formatted SD card into the QEMU boot
test, so such a test would be skipped on every CI run and would only exercise
the driver on the rare occasion a human set it up — which is to say, never.

So `tests.rs` **constructs** a 2 MiB F2FS volume byte by byte: superblock and
backup superblock, two checkpoint packs, two NAT copies, a root directory with
inline dentries, a subdirectory with a real dentry block, an inline-data file,
a two-block file, a sparse file, a symlink, a file with a 47-byte name
occupying six directory slots, and an `/big.bin` whose blocks reach all five
levels of the block path (inline addresses, both direct nodes, the indirect
node and the double-indirect node). It then mounts that image through
`fs::blocksrc::MemorySource` and drives the *real* driver over it — the same
code path a real device takes, differing only in where the bytes come from.

`/big.bin` is the reason the image can stay at 2 MiB while still reaching the
double-indirect level: it is nominally ~8.5 GiB and almost entirely holes,
with real blocks placed only at the five indices that matter. A test that
needed a genuinely 8.5 GiB image could not run in RAM at boot, and a test that
skipped the double-indirect level would leave the deepest and least-exercised
arithmetic in the driver unverified.

### Decision 3 — a valid-but-wrong answer is parked at each of the three indirections

This is the decision that most shaped the test image, and the one with the
most genuine alternative: it would have been much easier to build a volume in
which the three indirections all point at the *same* place, or in which the
wrong turns lead to garbage. Both would pass. Neither would prove anything.

If the wrong branch contains garbage, a driver that takes the wrong branch
fails — but it fails on the garbage, so the test proves only that the driver
noticed garbage, not that it chose correctly. If the wrong branch contains
*the same answer*, a driver that takes the wrong branch **passes**. Either way
the test does not distinguish a driver that resolves the indirection from one
that does not.

So each indirection gets a decoy that is structurally valid and semantically
wrong:

| Indirection | The wrong turn | What is parked there |
|---|---|---|
| Which checkpoint pack is current | Take the first pack, or the one with the higher CRC-valid tail, instead of the higher **version** | Pack A is a complete, CRC-valid checkpoint at version 3; pack B is version 7. They also disagree in *summary layout* — A is compacted (`CP_COMPACT_SUM_FLAG`), B is normal — so a driver that picks the wrong pack also reads its journal from the wrong offset and format |
| Which NAT copy the bitmap selects | Ignore the checkpoint's NAT bitmap and always read copy 0 | NAT copy 0 is entirely `NULL_ADDR`, so a driver that ignores the bitmap resolves every node to "hole" and fails loudly rather than subtly |
| Whether the NAT journal overrides the NAT area | Read the NAT area and stop | `hello.txt`'s NAT-area entry points at a **valid, well-formed, correctly-footered inode** — just a stale one, with different contents. Only the journal supersedes it. The test asserts the stale inode really is present at byte level, so the check cannot silently degrade into "there was nothing there anyway" |

The third row is the one that matters most, because it is the only one where
the wrong answer is indistinguishable from the right answer by any local
validity check: the stale inode passes its footer check, its `ino == nid`
check, and every bounds check the driver applies. Nothing but honouring the
journal gets you the right file. That is exactly the class of bug that ships.

The same logic drives the `test_corruption` group's split between *fatal* and
*survivable* damage. A blank device, a 512-byte device, both superblocks
destroyed, and both checkpoint packs torn must all fail to mount. But a torn
*newer* pack must **succeed** — falling back to pack A, and then correctly
reading its compacted journal, which is the second journal format and would
otherwise never be exercised. A destroyed *primary* superblock must succeed
via the backup at 5120. And damage confined to one file (a wrong footer nid, a
wrong footer ino, a pointer past the end of the volume, a pointer into the NAT
area, an absurd `current_depth`) must produce an error *for that file* while
leaving its neighbours readable — a driver that responds to one bad inode by
failing the whole mount is as wrong as one that ignores it.

### Decision 4 — a pack is valid only if its head and tail versions agree, and that is checked before the CRC

F2FS writes a checkpoint pack as a run of blocks, version number first and
last. The naive validity test is the CRC. The CRC is not sufficient: a pack
that was half-written when power failed can have a **perfectly valid CRC on
its header block**, because that block was written completely — it is the
*later* blocks that never landed. Comparing the version in the first block
with the version in the last block is what catches this, and it is why the
driver does that comparison and not only the checksum.

**Alternative considered:** trust the CRC alone, which is simpler and is what
a first-pass implementation naturally writes. Rejected because the failure it
misses is precisely the one checkpoints exist to protect against. The
`test_checkpoint` group asserts the head/tail rule directly: it tears only the
*tail* of the newer pack, leaving its header and CRC intact, and requires the
driver to fall back to the older pack.

### Decision 5 — the root inode is read at mount, not at first use

`open_source` reads the root inode and refuses the mount if it cannot be read
or is not a directory. Linux's `f2fs_fill_super` does the same thing, but the
reason is worth stating independently, because the cheaper alternative — do
nothing at mount, let the first `open()` discover the problem — is what the
driver originally did and it looked perfectly reasonable.

It is not, and the self-test is what showed why. The `test_corruption` group
zeroes the checkpoint's NAT bitmap so that every node id resolves through the
decoy NAT copy, and tears the older pack so no fallback repairs it. That is a
volume on which *nothing* is reachable. With no mount-time check the mount
**succeeded**: the driver had parsed a valid superblock and a valid
checkpoint, which is all it looked at.

To be precise about what that costs, because it is easy to overstate: the
volume does not then present as an *empty* directory — every operation on it
returns an error, so no caller is fooled into thinking it holds nothing. The
damage is the other two things.

First, the mount **is a claim**. A successful mount publishes a VFS entry,
shadows whatever directory sat at the mount point, and tells every subsequent
caller the volume is usable. Returning success for a volume we have not
established is readable makes the kernel assert something it never checked and
cannot honour, and it hides whatever was underneath the mount point behind a
filesystem that cannot answer anything.

Second, it **destroys attribution**. The user finds out from an `open()` or a
`readdir()` somewhere later that has nothing obviously to do with mounting,
long after the operation that actually had the evidence in hand. A fault
reported where it can be diagnosed is worth a great deal more than the same
fault reported later, and the mount is the only place in this sequence that
knows it was a *mount* that went wrong.

**Alternative considered:** validate lazily, which is genuinely cheaper (one
fewer block read per mount) and keeps `open_source` free of any dependency on
the node/inode layer. Rejected: one block read is nothing against a mount, and
the layering objection is backwards — a filesystem that cannot produce its own
root is not a filesystem that mounted.

A second self-test case pins the type half of the check specifically: a root
inode that reads perfectly, sits at a valid address, and is genuinely the one
the checkpoint names — but whose mode says regular file. Nothing the footer or
bounds checks understand is wrong with it. Only a check on the root's *type*
catches it.

### Cost

The read-only decision costs nothing to reverse — the write side is additive
and none of the read structures would need to change shape. The synthetic
volume is ~1000 lines of builders in `tests.rs` and runs in a few milliseconds
at boot. The decoys cost nothing at all beyond choosing different constants:
NAT copy 0 could have been a copy of copy 1, the two packs could have shared a
version, and `hello.txt`'s stale entry could have pointed at the same inode.
Making them differ is the entire value of the suite, in the same way that
Btrfs's self-test (§215) placing logical and physical addresses a constant
`0x08_0000` apart is the entire value of *its* suite.

The root-inode check is one extra block read per mount and about ten lines.

### Two real bugs this found

The checkpoint's own `checksum_offset` field was being read from byte 104
instead of byte **164**. At 104 sits `elapsed_time`-adjacent data that happens
to be small and plausible, so the driver did not obviously break — it computed
its CRC over the wrong span and rejected valid checkpoints intermittently,
depending on the volume. `cp.rs` now reads offset 164 and additionally
enforces `CP_MIN_CHKSUM_OFFSET = 192`, so a checksum offset pointing *into the
fixed header* is rejected rather than used, which `test_checkpoint` asserts.

The second is Decision 5's: the mount did not read the root inode, so a volume
on which nothing at all was reachable mounted successfully. It was found by the
corruption group rather than by reading the format, and it is the better
argument of the two for building the fake volume at all — no amount of care
re-reading Linux's on-disk structures would have surfaced it, because the bug
was not in a structure. It was in what the driver declined to look at.

Asking the same question of the other three filesystem ports found the gap in
**two** of them: `fs::btrfs` mounted having read only the *root* tree's
`ROOT_ITEM`, never touching a block of the FS tree it names; `fs::ntfs` mounted
having read MFT record 0 at the LCN the boot sector names, never reading record
5 through the `$MFT` runlist that every other record depends on. Only
`fs::iso9660` was already correct, and by accident — it reads the root extent
because it needs those bytes to sniff for Rock Ridge. Both gaps are now closed
the same way, each with two self-test cases (root unreachable; root readable
but not a directory). Three of four ports having the same hole is why this is
recorded as a class rather than as three bugs.
