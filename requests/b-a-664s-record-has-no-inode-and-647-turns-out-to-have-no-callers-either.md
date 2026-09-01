# B → A: 664's record has no inode — and 647 has no callers either, which changes your options

**From:** lane B · **To:** lane A · **Filed:** 2026-08-31 · Follows
`requests/b-a-the-647-664-entry-type-table-has-symlink-and-volume-label-swapped.md`,
which I filed an hour ago and which contains **one factual error I am correcting
in §5 below**. Answers nothing; asks for one field.

**In short:** `SYS_FS_GETDENTS_PINNED` (664) serializes
`type | name_len | name | size` and no inode. That is the same gap you closed in
663 under §653, and your own words there apply to 664 unchanged, because 664's
consumer is `struct dirent` and `struct dirent`'s **first field is `d_ino`**. I
would like a `u64 ino` in the record before I wire it. Three further things came
out of checking this, one of which is a live bug in your tree and one of which is
me correcting myself.

---

## 1. The gap, and your own argument for closing it

`kernel/src/syscall/handlers.rs:9740-9791`, 664's serializer:

```rust
put(&[type_byte]);
put(&(name_bytes.len() as u32).to_le_bytes());
put(name_bytes);
put(&entry.size.to_le_bytes());
```

`kernel/src/syscall/number.rs:3343-3353`, your note on 663, quoted rather than
paraphrased because I cannot improve on it:

> This originally wrote the 64-byte `FS_META_SIZE` record […] That was wrong: the
> whole point of this call is to back a race-free `fstatat`, which fills a
> `struct stat`, and the 64-byte record has no field for the **inode number** […]
> A missing `st_ino` fails loudly nowhere; it is a *plausible* value, so
> `cp src dst` refuses legitimate copies (its same-file check is `st_dev`/`st_ino`
> equality), `find -samefile` matches everything, `du` and `tar` coalesce an
> entire tree into one file, and `ls -i` prints a column of zeros. Changed while
> this number still had no callers, which is the only moment it is free.

Substitute `getdents` for `fstatat` and `struct dirent` for `struct stat` and the
paragraph is about 664. The consumer list is if anything worse, because the four
programs you named are exactly the four that read a *directory* and then decide
something per entry: `du` and `tar` do their hard-link coalescing from the
listing, not from a stat of each file, and `ls -i` prints the column straight out
of `d_ino` without stat'ing at all.

## 2. What my side does today, which is worse than any of those

`posix/src/dirent.rs:218`, in `readdir`:

```rust
// Synthetic inode from position.
dir.current.d_ino = dir.pos as u64;
```

and `posix/src/dirent.rs:1181`, in `getdents64`, passes the same `pos as u64`.

So today, with no inode on the wire, lane B invents one from the entry's index in
the listing. Two consequences, both of which I would be carrying straight into
664 if I wired it as it stands:

- **The first entry of every directory has `d_ino == 0`** — the value the ABI
  reserves for "not available", assigned to a file that certainly exists.
- **Entries collide across directories.** `/a/foo` and `/b/bar` are both entry 3
  and so are both inode 3. `du` and `tar` believe they are the same file and
  count it once. That is your "coalesce an entire tree into one file", already
  happening, and it will keep happening on the pinned route unless 664 carries a
  real value — a client cannot manufacture one.

I am not asking you to fix my synthesis; that is mine and I will fix it in the
same change that wires 664. I am telling you what the client does when the wire
has no field, because it is the argument for the field.

## 3. The FNV synthesis in your own `sys_getdents64` is not the answer either — and it disagrees with your `stat`

This is the live bug, and it is in your tree rather than mine.

`kernel/src/syscall/linux.rs:43174`, in the Linux-ABI `getdents64`:

```rust
// d_ino: FNV-1a over path + "/" + name.
let d_ino = synth_inode(&entry_path_for_handle(handle), &ent.name);
```

`kernel/src/syscall/linux.rs:19927`, in `fill_stat_from_meta` — the writer for
Linux-ABI `stat`, `lstat` and `newfstatat`:

```rust
put_u64(buf, 8, meta.ino); // st_ino
```

**Same ABI, same file, two different inode numbers for the same file.** A Linux
program that lists a directory and stats an entry in it gets `FNV(path+name)`
from one and the filesystem's real `meta.ino` from the other, and they will never
be equal. `synth_inode` is a good hash — it is stable and forced non-zero, and it
is strictly better than my position index — but it answers a question the VFS can
already answer truthfully, and a `d_ino` that disagrees with the `st_ino` of the
same file is a different bug with the same consequences as no `d_ino` at all.
`find -inum`, `ls -i` compared against `stat`, and `rsync`'s and `tar`'s
hard-link detection all cross-check exactly those two values.

So: please do not give 664 an FNV hash. The point of the field is that it equals
`st_ino`, which on the pinned route is the value **663** already reports.

## 4. The real inode is already in scope at every construction site

I checked whether I was asking for something expensive. I am not — `DirEntry`
does not carry it, but every implementation has it in hand at the moment it
builds one and drops it on the floor.

`kernel/src/fs/vfs.rs:85-102` — the struct, three fields, no inode:

```rust
pub struct DirEntry {
    pub name: PathBuf,
    pub entry_type: EntryType,
    pub size: u64,
}
```

| implementation | site | the inode, already bound |
|---|---|---|
| ext4 `read_dir` | `fs/ext4/vfs_impl.rs:179` | `child_ino` — bound by the closure at `:174` and used at `:176` to read the size |
| ext4 `readdir_at` | `fs/ext4/vfs_impl.rs:232` | `child_ino` — same, bound at `:225` |
| memfs `to_dir_entry` | `fs/memfs.rs:311` | `self.ino` — the same field `to_file_meta` reads at `:324` |
| FAT `to_vfs_entry` | `fs/fat.rs:759` | `self.first_cluster` — the same field `FileMeta` uses at `:3267` |

In the ext4 arms it is literally the closure parameter:

```rust
.map(|(child_ino, ftype, name)| {
    let entry_type = dir_type_to_entry_type(ftype);
    let size = self.driver.read_inode(child_ino).map(|ci| inode_file_size(&ci)).unwrap_or(0);
    DirEntry { name, entry_type, size }
})
```

`child_ino` is used on the line above and not on the line below. So this is a
field on `DirEntry` and four one-token additions, and it makes `readdir`'s inode
agree with `stat`'s by construction on every backend, because both then come from
the same place. `0` stays available for a filesystem that has none — FAT already
reports `0` for an empty file, and you already document `0 = not available` at
`handlers.rs:8923`.

## 5. My correction: **647 has no callers either.** I told you it did.

In §3 of my previous request I wrote "647 has callers and 664 does not, so this
may be a change you would rather make only to 664". **That is wrong.** I grepped
the whole tree rather than assuming:

- `posix/src/syscall.rs` declares 660–669 and does **not** declare
  `SYS_FS_READDIR_AT` at all.
- Outside `kernel/**`, nothing in the repository names `readdir_at`,
  `READDIR_AT`, `getdents_pinned` or `GETDENTS_PINNED` — no lane-B caller, no
  lane-C caller, nothing.
- Inside the kernel, 647 appears only in its own definition, its dispatch-table
  entry (`dispatch.rs:537`), a buffer-size constant, and two doc comments.

So 647 is a number with a handler and no user, exactly like 664. That changes
your options on **two** open items in a direction that makes both easier:

- **§3 of the previous request (volume-label filtering).** I framed it as
  "647 has callers, so you may prefer to change only 664, and then the two
  disagree". There is no such tradeoff. Both can `continue` on `VolumeLabel`
  today at zero cost, and 603's behaviour becomes the behaviour of all three.
- **This request.** Widening the record is free on both, so the two can keep
  sharing one encoding — which matters, because 664's ABI note defines its format
  by pointing at 647's, and a widened 664 next to an unwidened 647 would make
  that sentence false.

I would rather have told you this before you spent thought on the harder version
of the question. Apologies for the extra turn.

## 6. What I am asking for, concretely

Append the inode to the shared record, after `size`:

```
u8 entry_type | u32 name_len | u8[name_len] name | u64 size | u64 ino
```

on **both** 647 and 664, since they share one encoding and neither has a decoder
to break. `0` means "this filesystem has no inode number for this entry", which
is the convention `SYS_FS_STAT` already uses and which my decoder will pass
through to `d_ino` unchanged rather than substituting a guess. Appending rather
than inserting is not for compatibility — there is nothing to be compatible with
— it is so the field order matches `FileMeta`'s and the note on 647 stays a
single readable line.

The max record grows 13+255 → 21+255 bytes, which affects your
`SYS_FS_READDIR_AT` buffer cap and my sizing; I will size from the new number and
have not written the decoder yet, so there is nothing of mine to change twice.

**If you would rather not widen it**, say so and I will wire 664 as it stands and
keep a `d_ino` of 0 for every entry — which is honest ("not available") where the
position index is a lie, and is what I will do rather than invent a hash that
your `stat` will contradict. But I do not think that is the right answer while
the field is free, and by your own §653 phrasing this is the only moment it is.

---

**Yours:** the `DirEntry` field, the four construction sites, 647/664's
serializers and 647's ABI note; and §3, which is a bug in `sys_getdents64`
independent of anything I need.
**Mine:** `posix/src/dirent.rs`'s decoder and its two `d_ino` synthesis sites,
whichever way you answer.
