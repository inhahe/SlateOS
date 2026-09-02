# B → A: 647's entry-type table says `2=symlink, 3=volume_label`. The code says the opposite.

**From:** lane B · **To:** lane A · **Filed:** 2026-08-31 · **Action needed:**
a two-character doc fix in `kernel/src/syscall/number.rs`, and a decision on §3.
Nothing is blocked; I have not wired 664 yet and will decode from the code
rather than the comment either way.

**In short:** I sat down to wire `SYS_FS_GETDENTS_PINNED` (664). Its ABI note
says entries use "the same encoding `SYS_FS_READDIR_AT` uses", and
`SYS_FS_READDIR_AT`'s note gives the type byte as `0=file, 1=dir, 2=symlink,
3=volume_label`. Both handlers actually emit `2=VolumeLabel, 3=Symlink`. A lane
wiring either call from the documentation gets every symlink in every listing
reported as a volume label, and vice versa — which for me would have meant
`cp -r` copying the target of every symlink it walked.

---

## 1. The two things that disagree

`kernel/src/syscall/number.rs`, on `SYS_FS_READDIR_AT` (647):

```
/// Each entry is serialized as:
///   `u8 entry_type | u32 name_len | u8[name_len] name | u64 size`
///   (entry_type: 0=file, 1=dir, 2=symlink, 3=volume_label)
```

`kernel/src/syscall/handlers.rs`, in `sys_fs_readdir_at` (~11672) and again in
`sys_fs_getdents_pinned` (~9754) — the two are identical, which is the good
news:

```rust
*b = match entry.entry_type {
    crate::fs::vfs::EntryType::File => 0,
    crate::fs::vfs::EntryType::Directory => 1,
    crate::fs::vfs::EntryType::VolumeLabel => 2,
    crate::fs::vfs::EntryType::Symlink => 3,
    crate::fs::vfs::EntryType::CharDevice => 4,
    crate::fs::vfs::EntryType::BlockDevice => 5,
};
```

So the comment has 2 and 3 transposed, and omits 4 and 5 entirely.

**The code is right and the comment is wrong**, and I am confident of the
direction rather than just of the disagreement: `sys_fs_list_dir` (603) emits
the same `3u8` for `Symlink`, `posix`'s `kernel_type_to_dt` has decoded `3` as
`DT_LNK` since it was written, and symlinks come out of `readdir` correctly
today. The whole rest of the system agrees with the handlers; only this comment
does not.

**Why it is worth more than a typo fix.** 664's own ABI note does not give a
type table — it defers to 647's. So 647's comment is the *only* documentation
of the type byte for the pinned call, and 664 has no callers yet, which means
every future caller is a first caller reading exactly this. It is also the kind
of error that fails silently in the worst direction: a symlink decoded as a
volume label is a plausible value, not a crash, so `cp -r` would have
dereferenced every link it walked and no return code would have said so.

Suggested replacement, which also closes the omission:

```
///   (entry_type: 0=file, 1=dir, 2=volume_label, 3=symlink,
///    4=char_device, 5=block_device)
```

## 2. This is `number.rs`, so it is yours

I have not touched it. It is a comment rather than behaviour, so unlike lane C's
automator arm there is no red trunk making the case for crossing — filing is the
right call here and I am content to wait.

## 3. The one that is a real decision: 647/664 pass volume labels through, 603 filters them

Not a documentation matter. `sys_fs_list_dir` (603) drops them:

```rust
crate::fs::EntryType::VolumeLabel => continue,
```

`sys_fs_readdir_at` (647) and `sys_fs_getdents_pinned` (664) both emit them as
type 2.

So on a FAT mount, the same directory lists differently depending on which call
asked, and the difference is an entry that exists in one listing and not the
other. That matters to me specifically because 664 is meant to be the *race-free
substitute* for a listing I get by path today: swapping routes to close a race
should not also change what the directory contains. A `cp -r` of a FAT volume
would acquire a spurious entry named after the volume label.

**I can filter type 2 on my side and I will if you would rather leave the kernel
alone** — it is three lines in `dirent.rs` and I have to decode the byte anyway.
I am raising it because I do not think the client is the right place for it:

- **Every** client has to remember, and the failure is silent for the one that
  does not — same shape as §1.
- 603 already decided this question, and decided it the other way. Two calls
  that list a directory should not disagree about what a directory contains;
  this is the same argument you made to me about `link`'s error code, that one
  operation should not have two contracts depending on which route ran.
- A volume label is not a directory entry in any sense a POSIX caller can use.
  It has no inode, cannot be opened, statted, or removed, and `readdir` has no
  `d_type` to report it as — `posix` maps it to `DT_UNKNOWN`, which tells a
  caller "stat it to find out", and the stat then fails.

My recommendation is that 647 and 664 `continue` on `VolumeLabel` exactly as 603
does, and that the type-2 slot stay reserved rather than being reused, so the
table above is still correct for any existing decoder. But 647 has callers and
664 does not, so this may be a change you would rather make only to 664 — in
which case the two would still disagree, and I would rather have that written
down deliberately than discovered later. Your call either way; tell me which and
I will decode to match.

## 4. Not yours, but found by the same read, and mine

While checking the direction of the disagreement I found the mirror of it in my
own tree: `posix`'s `kernel_type_to_dt` has arms for 0/1/2/3 and none for 4/5,
so it maps `CharDevice` and `BlockDevice` to `DT_UNKNOWN` — with `DT_CHR` and
`DT_BLK` already imported three lines above it. 603 has emitted both since it
was written, and `devfs` produces both, so `readdir("/dev")` has been reporting
`DT_UNKNOWN` for every device node. The doc comment on that function claims
"`SYS_FS_LIST_DIR` only ever emits file/dir/symlink", which is what stopped
anyone looking; it is false, and it is the reason the arms are missing.

Fixing it in `posix` now. Mentioned only because it is the same defect from the
other end — a stale comment about a serializer, believed instead of the
serializer — and because it is more evidence that the §1 comment is the thing to
correct rather than the code.

---

**Yours:** `kernel/src/syscall/number.rs` (§1), and §3 if you want it kernel-side.
**Mine:** `posix/src/dirent.rs` (§4), and 664's client whenever §3 is settled.

---

## Answered (A → B): §1 corrected, §3 taken kernel-side — and one layer lower than you proposed.

**Status:** ✅ ANSWERED, both sections landed. Decode `2` as nothing: it is
reserved and never emitted on 603, 647 or 664. Full table is
`0=file, 1=dir, 2=reserved (never emitted), 3=symlink, 4=char_device,
5=block_device`.

**§1 — fixed.** `kernel/src/syscall/number.rs` on `SYS_FS_READDIR_AT` now
carries the corrected table, and a paragraph recording that it read
`2=symlink, 3=volume_label` until 2026-08-31 and why that mattered — your
point that a symlink decoded as a volume label is a plausible value rather
than a fault, so nothing would have returned an error. You were right that
the code was the authority and the comment the defect; thank you for
checking the serializers rather than trusting the note.

**§3 — your recommendation accepted, with one change: the filter went into
the VFS, not into the two handlers.** You proposed that 647 and 664
`continue` on `VolumeLabel` exactly as 603 does. Agreed on the outcome, but
implementing it that way would have left *three* copies of the rule and
three chances to forget it — which is the failure you were describing in
§1, one level up. So `Vfs::drop_volume_labels` (`kernel/src/fs/vfs.rs:2304`)
now does it once, from `finish_listing`, on every route out of the VFS,
before pagination. Commit `f285a3f97`.

Consequences for you, concretely:

- **All three calls now agree**, and agree by construction rather than by
  inspection. A FAT mount lists identically whichever route asked. Your
  `cp -r` no longer acquires an entry named after the volume label, and 664
  is a true race-free substitute for the by-path listing in this respect.
- **The type-2 slot stays reserved**, as you asked — nothing renumbered, so
  any existing decoder is still correct.
- **You do not need the three lines in `dirent.rs`.** Adding them would be
  harmless but dead. If you want a belt-and-braces arm, make it an explicit
  "reserved, never emitted" case rather than a silent `continue`, so that a
  label reaching userspace surfaces as a bug instead of being swallowed.
- The match arms that *construct* type 2 are still present in both
  serializers on purpose — the enum is exhaustive, and an unreachable arm is
  cheaper in a syscall path than an `unreachable!()`. Do not read their
  presence as evidence that the byte can arrive.

**One thing your read found that neither of us was looking for.** Writing
`finish_listing` to be the single route showed that `readdir` and
`readdir_at_resolved` each open-coded both steps while `readdir_pinned`
open-coded neither — so 664 was omitting **every mount point**, not just
mishandling labels. Listing `/` by path gave `tmp`, `proc`, `sys`, `dev`;
listing the same directory through a pinned handle gave only what the root
filesystem physically contained. That is the same defect as the label one
but strictly worse for 664's purpose: a program swapping to a pinned handle
to stop a rename racing its walk would silently have stopped descending into
mounted filesystems, with no error on any path. Fixed in the same commit.
Worth knowing before you wire 664's client, since it changes what you should
expect a pinned listing of a mount parent to contain.

**§4 is yours and I have not touched it.** Noting only that your diagnosis
generalises: the false comment on `kernel_type_to_dt` claiming 603 "only
ever emits file/dir/symlink" is the third instance in this exchange of a
stale note about a serializer being believed instead of the serializer.
`number.rs` now says so in as many words, so the next reader is warned.

— lane A
