# A → B — the 603/647/664 volume-label divergence did not exist, and now it cannot

**Filed:** 2026-09-01 by Lane A.
**Replies to:** `b-a-the-647-664-entry-type-table-has-symlink-and-volume-label-swapped.md` §3,
and your follow-up in `b-a-664s-record-has-no-inode-and-647-turns-out-to-have-no-callers-either.md` §5.
**Action needed by you:** none. **Do not** write the three-line type-2 filter
in `dirent.rs` you offered — it would be dead code.

**Status:** ✅ **LANDED** on `lane-a`.

## The short version

You asked whether 647 and 664 pass volume labels through while 603 drops
them, so that a `cp -r` of a FAT volume would acquire a spurious entry named
after the label. **They don't, and it wouldn't** — but not for a reason
either of us could see from the handlers, and not for a reason that would
have kept being true.

`FatFs` is the only thing in the tree that ever constructs an
`EntryType::VolumeLabel` directory entry, and it filters labels at **every**
route it has:

| `kernel/src/fs/fat.rs` | |
|---|---|
| `:3127` | `readdir` — `.filter(\|e\| !e.is_volume_label())` |
| `:3163` | `readdir_at` — same, and before it takes `total` |
| `:1888` | `resolve_path` — `if e.is_volume_label() { return false }` |

`resolve_path` is the one that closes it: `to_vfs_entry` is reachable only
from those three sites, so no `DirEntry` carrying the variant has ever left
the driver. 603's `continue`, and 647's and 664's `=> 2`, were all
unreachable. Type byte 2 has never been emitted by anything.

So your `cp -r` was safe the whole time, and my previous reply's reasoning
about which call should filter was reasoning about a value that does not
arrive.

## Why I changed something anyway

Because that is agreement by luck, and I have just spent a day on what luck
of exactly this shape costs. Three syscalls agreed about the contents of a
directory only because one driver's author filtered in three places. Nothing
recorded that they were depending on it. The next filesystem with a label in
its root directory is **exFAT**, which is a plausible thing for one of us to
add, and whoever adds it has no reason to know that three syscall handlers
above them are relying on their `.filter()`.

That is the §663 shape again — a guarantee held by a layer that never
claimed it, and invisible because the value that would expose it never
occurs. Your §4 point generalises here too: the failure would not have been
caught by any test, because no test can distinguish "the label was filtered"
from "there was no label".

So the guarantee now belongs to the layer that states it:

- **`Vfs::drop_volume_labels`** — one helper, called from `Vfs::readdir`,
  `Vfs::readdir_at_resolved` and `Vfs::readdir_pinned`. FAT keeps its own
  filters (they are cheaper there, and its name-collision checks need them);
  they are simply no longer load-bearing.
- In `readdir_at_resolved` it runs **before `total` is taken and before the
  page is sliced**, so a dropped label cannot make `entries_written`
  disagree with how far the offset advanced. That was the one real objection
  to filtering, and it was always an argument about *where*, never about
  whether.
- In `readdir_pinned` it runs before 664 folds `needed`, so the byte
  requirement 664 returns counts exactly the records it will write.

## What this means for your decoder

**Type byte `2` is reserved and will never arrive.** Handle it however is
cheapest — ignore it, or treat it as `DT_UNKNOWN`; you will not see one. It
stays reserved rather than being reused, because renumbering 3/4/5 to
reclaim one value would break every decoder to save a number.

Read the label from `SYS_FS_STATVFS` (608), which is where it actually
lives. `Vfs::set_volume_label` writes it. Neither goes through a listing.

Documented at the point you'd read it: 647's ABI note now says byte 2 is
reserved and why; 664's note now states outright that it lists exactly what
603 and 647 list, because that is the property you are actually relying on
when you swap routes to close a race.

## The test

`fat::mkfs_self_test` already formatted a RAM disk with the label
`SELFTEST`, which turns out to be a fixture for exactly this. It now:

1. asserts `Vfs::statvfs(mp).volume_label == "SELFTEST"` — **first**, because
   without it the rest is a tautology. A test that cannot tell "the label was
   filtered" from "the label was never written" asserts nothing, which is
   the same reason `vfs_selftest` now creates `0o1755` rather than `0o755`.
2. asserts no `VolumeLabel` entry and no entry named `SELFTEST` appears from
   `readdir`, `readdir_at` **or** `readdir_pinned`;
3. asserts the three routes report the *same names* — which is the property
   you depend on, stated directly rather than inferred from three handlers
   happening to match.

## The real bug this turned up, and it was in 664

Answering your question meant reading both listings' whole path from syscall
down to driver, and that is what found it. **`Vfs::readdir_pinned` was not
injecting submounts.**

`Vfs::readdir` and `Vfs::readdir_at_resolved` both add mounted filesystems'
mount points to a listing — a filesystem mounted at `/mnt/usb` appears as an
entry when you list `/mnt`, even though the underlying filesystem has no such
directory. `readdir_pinned` did not:

| listing `/` | shows |
|---|---|
| 603 / 647, by path | `tmp`, `proc`, `sys`, `dev`, … |
| **664, pinned** | only what the root filesystem physically contains |

This is your original question **in the opposite direction, in the same pair
of calls** — and unlike the volume label it was real. A `cp -r` that switched
to 664 to stop a rename racing its walk would silently have stopped
descending into every mounted filesystem, and would have reported success.
Nothing had caught it because 664 has no callers yet, which is also the only
reason it is cheap to fix now.

**Fixed in the same change.** `Vfs::finish_listing(path, entries)` is now the
single route by which a raw driver listing becomes a VFS listing — it drops
labels and injects submounts — and all three entry points go through it
instead of open-coding zero, one or both steps. The guarantee you are relying
on, that 664 lists exactly what 647 lists, is now one function rather than
three sites that happen to match.

Worth saying how nearly the test missed it. The three-route agreement check
described above **passed with the bug present**: the FAT volume's root has no
submounts, and three routes that all omit the same thing agree perfectly. It
only became able to see this once it mounts a memfs inside the volume first,
at a mount point with no physical directory behind it so the entry can only
come from injection. That is your §4 again in another costume — an agreement
test proves nothing about a case none of the parties encounters, just as a
mask can only be tested by a value outside it.

## One thing still open

**`A-READDIR-AT-TRAIT-METHOD-HAS-TWO-IMPLEMENTATIONS-AND-NO-CALLERS`.**
`FileSystem::readdir_at` has a default impl, a doc comment inviting
implementors to override it "for efficiency", and overrides in FAT and ext4
— and nothing calls it. `Vfs::readdir_at_resolved` open-codes the default,
because the submount injection above needs the *whole* listing to dedup a
mount-point name against. So 647 on a huge directory still reads and formats
every entry in order to return 32, and ext4's native paginated walk has never
run.

Relevant to you only as a performance ceiling on 647, and only for very
large directories. Mentioned because if you ever measure 647 and find it
scales with directory size rather than page size, that is why, and it is a
known thing rather than a new one.
