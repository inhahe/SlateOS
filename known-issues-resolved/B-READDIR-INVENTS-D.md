## B-READDIR-INVENTS-D_INO-FROM-THE-ENTRY-POSITION-SO-DU-AND-TAR-COALESCE-A-TREE — **FIXED 2026-09-01, closed 2026-09-12**

**The code was fixed eleven days before this entry said so, and that gap is
the interesting part.** Both sites named in the table below now carry the
kernel's inode verbatim:

| site | code today |
|---|---|
| `readdir` | `dir.current.d_ino = entry.ino;` |
| `getdents64` | `emit_linux_dirent64(remaining, entry.ino, …)` |

`entry.ino` comes from the last eight bytes of the shared 647/664 record,
which lane A widened to
`u8 type | u32 name_len | u8[name_len] name | u64 size | u64 ino` — the whole
of what this entry asked for. `d_ino == 0` now means what the ABI says it
means, and two files in different directories no longer claim the same
identity, so `du` and `tar` stop coalescing a tree.

**Why it stayed open.** The fix landed inside `e99428c0a`, *posix: every
directory stream reads through its descriptor, via 664* — a commit about
routing directory reads through a descriptor, which had to parse the new record
to do its own job and picked up the inode on the way. Nobody was closing this
entry because nobody was working on this entry. **A fix that arrives as a side
effect of a different change has no natural moment at which its bug gets
closed**, which is a third distinct way for a document to go stale, alongside a
blocker cleared by another lane and a file that has been retired.

Found by `scripts/check-stale-blockers.py`, which flagged it for citing a
request that reports itself LANDED. The checker could not know the code was
already right — only that the entry's stated reason for waiting had expired,
which was enough to make someone look.

`cargo test -p posix --target x86_64-pc-windows-gnu dirent`: 97 passed, 0
failed.

The description below is kept in the tense it was written in.

**In short:** when a program lists a directory, every entry comes back with an
"inode number" — the number the filesystem uses to tell one file from another.
Ours does not come from the filesystem. It is the entry's position in the
listing: the first file in a directory is 0, the second is 1, and so on. So the
first file in *every* directory claims the number that means "unknown", and the
third file in one directory and the third file in another claim to be the same
file. `du` and `tar` use exactly that number to notice when two names are one
file, so they count such a tree once instead of once per file. Blocked on lane A:
the kernel does not send an inode for a directory entry, and a client cannot
invent a true one.

**Where.** `posix/src/dirent.rs`, two sites, both feeding the same wrong value:

| site | code | what it fills |
|---|---|---|
| `readdir`, `:218` | `dir.current.d_ino = dir.pos as u64;` | `struct dirent`'s `d_ino` |
| `getdents64`, `:1181` | `emit_linux_dirent64(remaining, pos as u64, …)` | the `d_ino` of each `linux_dirent64` |

The comment at `:217` says "Synthetic inode from position", so this was known to
be a placeholder when written; what was not recorded is that the placeholder is
observable and that two of its values are actively wrong rather than merely
uninformative.

**The two failures, in order of how quietly they fail.**

1. **`d_ino == 0` for the first entry of every directory.** Zero is the value the
   ABI reserves for "no inode available"; `kernel/src/syscall/handlers.rs:8923`
   documents it as such for `SYS_FS_STAT`'s record. Assigning it to a file that
   certainly exists means a caller that checks for the reserved value gets the
   wrong answer about the one entry it is most likely to look at first.
2. **Collisions across directories.** `/a/foo` and `/b/bar` are both the third
   entry of their directory and so both inode 3. `du` and `tar` do hard-link
   coalescing from the *listing* rather than from a stat of each file, so they
   see one file with many names and count it once. `find -samefile` matches on
   position. `ls -i` prints a column of indices. This is precisely the failure
   list lane A wrote when they closed the same gap in `SYS_FS_FSTATAT_PINNED`
   (`kernel/src/syscall/number.rs:3343-3353`, design-decisions.md §653) — it is
   already happening here, one call over.

**Why it is not fixable on this side.** The client has nothing better to use.
Neither directory-listing wire format carries an inode: `SYS_FS_LIST_DIR` (603)
writes a 264-byte record of `name[256] | size[4] | type[1] | pad[3]`, and
`SYS_FS_READDIR_AT` (647) / `SYS_FS_GETDENTS_PINNED` (664) write a packed
`type | name_len | name | size`. The only way to get a true inode per entry today
is one `stat` syscall per entry during the walk, which turns an O(1) listing into
O(n) syscalls and still races — the entry can be replaced between the listing and
the stat, which is the very race 664 exists to close.

**What the proper fix looks like.** A `u64 ino` appended to the shared 647/664
record, requested in
`requests/b-a-664s-record-has-no-inode-and-647-turns-out-to-have-no-callers-either.md`.
It is cheap kernel-side: `DirEntry` (`kernel/src/fs/vfs.rs:85-102`) has no inode
field, but every implementation has the value in hand at the moment it builds one
— ext4's `child_ino` is the closure parameter at `fs/ext4/vfs_impl.rs:174`/`:225`
and is used on the line above the construction, memfs has `self.ino`
(`fs/memfs.rs:311`), FAT has `self.first_cluster` (`fs/fat.rs:759`). When the
field lands, both sites above read it from the wire and pass `0` through
unchanged rather than substituting anything.

**What NOT to do, recorded because it is the tempting fix.** Do not hash the name
into an inode client-side. The kernel's own Linux-ABI `getdents64` does exactly
that — FNV-1a over path + "/" + name, `kernel/src/syscall/linux.rs:43174` — and
it is a bug rather than a model: `fill_stat_from_meta` at `:19927` writes the
real `meta.ino` as `st_ino`, so within one ABI a `stat` and a `getdents64` of the
same file report two different inodes, and every consumer that cross-checks the
two (`find -inum`, `ls -i` against `stat`, `rsync`/`tar` hard-link detection)
sees a contradiction instead of a missing value. A synthesized inode that
disagrees with `st_ino` is not an improvement on a missing one; it converts a
detectable gap into an undetectable disagreement. Reported to lane A as §3 of the
request above.

**Until then**, the honest interim is `0` for every entry rather than the
position index — "not available" is true where "entry 3" is false. Not applied
yet, because it would regress the one thing the index accidentally gets right
(distinct entries within a single directory compare unequal, which is what makes
`ls -i` of one directory look plausible), and because the field is expected to
land before 664 is wired. If lane A declines the widening, apply the `0` and note
it here.
