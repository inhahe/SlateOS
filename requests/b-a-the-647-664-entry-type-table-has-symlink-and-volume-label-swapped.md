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
