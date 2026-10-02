## A-F2FS-MOUNT-DID-NOT-READ-THE-ROOT-INODE-SO-AN-UNREADABLE-VOLUME-MOUNTED-EMPTY (lane A, 2026-08-16) — FOUND AND FIXED

**Status: fixed** in `kernel/src/fs/f2fs/mod.rs` (`F2fsFs::open_source`), same
day as the port, before the driver ever ran against a real volume. Recorded
because the bug was not in any parsing routine — it was in what the mount
declined to look at — and that is a class the usual review passes do not cover.

### What was wrong

`open_source` read the superblock, read the checkpoint, and returned. It never
read the root inode. Both of those steps can succeed on a volume where every
subsequent node lookup fails, because the checkpoint's NAT bitmap selects which
of the two node-address-table copies is live, and a checkpoint that is
internally valid can still select a copy that is not.

### How it presented

Not as an error. The mount **succeeded** on a volume where nothing at all was
reachable.

Being precise about the cost, because it is easy to overstate: the volume does
not then look *empty* — every operation on it returns an error, so no caller is
misled into thinking it holds nothing. The damage is elsewhere, and it is real:

- **The mount is a claim.** A successful mount publishes a VFS entry, shadows
  whatever directory sat at the mount point, and tells every later caller the
  volume is usable. Returning success for a volume never established as
  readable makes the kernel assert something it did not check, and hides
  whatever was under the mount point behind a filesystem that cannot answer.
- **It destroys attribution.** The user finds out from an `open()` or
  `readdir()` somewhere later that has nothing obviously to do with mounting —
  minutes or hours after the one operation that had the evidence in hand and
  knew the context was a *mount*.

### How it was caught

By the self-test's corruption group (`kernel/src/fs/f2fs/tests.rs`,
`test_corruption`), which zeroes the live checkpoint's NAT bitmap so every node
id resolves through the decoy NAT copy — where every entry is `NULL_ADDR` — and
also tears the older pack so no fallback quietly repairs the volume. The check
asserted `mount_image(img).is_err()`. It returned `Ok`.

Worth being precise about what did and did not find this. Re-reading Linux's
on-disk structures — the technique that found the checkpoint checksum-offset
bug recorded above — could not have found this one, because the bug is not in a
structure. Only running the driver against a volume built to be unreadable
surfaced it. That is the strongest argument in the F2FS work for building a
synthetic volume at all, and it is recorded as such in `design-decisions.md`
§216 Decision 5.

### The fix

`open_source` now reads the root inode and refuses the mount with
`CorruptedData` if it cannot be read or if its mode says it is not a directory,
logging which of the two it was. This matches Linux's `f2fs_fill_super`, which
does `f2fs_iget(sb, F2FS_ROOT_INO(sbi))` and rejects a root that is not a
directory. Cost: one block read per mount.

Two self-test cases pin it: the NAT-bitmap case above (root unreachable), and a
second case that gives the root a perfectly valid inode whose mode is
`0o100644` — a valid node, at a valid address, genuinely named by the
checkpoint, wrong only in *type*, which nothing but a type check catches.

### The generalisable lesson

**Ask what a mount routine proves, not what it parses.** Parsing the superblock
proves the volume claims to be this filesystem; parsing the checkpoint proves
the metadata roots are self-consistent. Neither proves the filesystem has a
root you can reach — and "has a reachable root" is the actual precondition for
everything the mount goes on to promise. Any driver whose mount succeeds
without touching the object the whole namespace hangs from can report success
on a volume it cannot read.

### The same question, asked of the other read-only ports — two had it, one did not

Asked immediately rather than filed as a follow-up, because the lesson is worth
nothing unless it is applied:

- **`fs::iso9660` — already correct.** `Iso9660Fs::open` reads the root
  directory's extent (`read_extent_raw(device, pvd.root_dir_lba, …)`) in order
  to sniff for Rock Ridge, and propagates the failure with `?`. It gets the
  guarantee as a side effect of needing those bytes for another reason — but it
  gets it.
- **`fs::btrfs` — had the gap; fixed.** Mount ran all five bootstrap steps and
  ended at the FS tree's `ROOT_ITEM`, which lives in the *root* tree and only
  says where the FS tree is. Not one block of the FS tree was read. A volume
  whose FS tree root block is unreadable, stale (wrong generation), or
  mistranslated by the chunk map completed every step and mounted.
  `open_source` now looks up `fs_root.root_dirid` and requires a directory.
- **`fs::ntfs` — had the gap; fixed.** The bootstrap reads MFT record 0 at the
  LCN the boot sector names, which proves only that the *start* of `$MFT` is
  where it claims to be; the `$Volume` read at record 3 is deliberately
  best-effort and its failure is absorbed. Every other record — the root
  directory, record 5, included — is addressed through `$MFT`'s **runlist**, so
  a volume whose runlist is wrong past its first run mounted cleanly.
  `open_source` now loads record 5 and requires `is_directory()`.

Three of the four filesystem ports had the same hole, and the one that did not
avoided it by accident rather than by design. That ratio is why this is written
up as a class rather than as three separate bugs.
