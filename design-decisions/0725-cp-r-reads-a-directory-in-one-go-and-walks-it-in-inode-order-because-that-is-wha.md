## 725. `cp -r` reads a directory in one go and walks it in inode order, because that is what GNU's `savedir` does and `-v` made the difference visible

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous) · **Lane:** B

**In short:** Copying a folder means listing what is in it and copying each
thing. Until now our `cp` listed them in whatever order the filesystem handed
them back; GNU sorts the list first. Nobody could tell, because the copy comes
out the same either way. Then `cp -v` landed — the option that prints one line
per file as it goes — and suddenly the order is *printed*, and ours printed a
different one from GNU's. The decision is to sort the way GNU sorts, which also
happens to be the faster way to read a disk, at the cost of holding the whole
list in memory instead of one name at a time.

### What was measured

A directory holding `a.txt`, `sub` and `link`, created in the order `sub`,
`a.txt`, `link`, on ext4 under WSL:

```
$ cp -rv tree dst              $ ours -rv tree dst
'tree' -> 'dst'                'tree' -> 'dst'
'tree/sub' -> 'dst/sub'        'tree/a.txt' -> 'dst/a.txt'
'tree/sub/b.txt' -> …          'tree/link' -> 'dst/link'
'tree/a.txt' -> 'dst/a.txt'    'tree/sub' -> 'dst/sub'
'tree/link' -> 'dst/link'      'tree/sub/b.txt' -> …
```

GNU's order is creation order; ours was ext4's htree hash order. The cause is
`copy.c:834`, `savedir (src_name_in, SAVEDIR_SORT_FASTREAD)`. In gnulib's
`savedir.c` that option resolves to `direntry_cmp_inode` wherever
`D_INO_IN_DIRENT` is defined — an ascending sort on `d_ino` — and to no sort at
all where it is not. Inode numbers on ext4 are allocated in sequence, so
creation order and inode order coincide.

### The decision

`read_dir_fastread` in `userspace/coreutils/src/bin/cp.rs` collects the whole
directory into a `Vec<DirEntry>` and, under `#[cfg(unix)]`, sorts it by
`DirEntryExt::ino()`. Off Unix it does not sort, which is gnulib's own
fallback and not an approximation of it.

### Why, and what it costs

**For it.**

* *It is the only order that matches.* This program's contract is that its
  output cannot be told from GNU's, and `scripts/cp-diff.sh` is where that is
  certified. Without the sort, `run_case -rv tree dst` differs — and the honest
  responses to that are either to fix it or to mark an *implemented* option as
  differing on purpose. The second would put a lie in the summary line.
* *It is also the fast order, which is why gnulib calls it `FASTREAD` and not
  `SORTED`.* On any filesystem that keeps inodes in tables, inode number is
  roughly on-disk position, so reading entries in inode order turns the
  scattered seeks of one `stat` per entry into a forward scan. The gain is on a
  cold cache and is exactly the reason upstream does it.
* *The list had to be materialised anyway* the moment we wanted to sort, and
  materialising it is separately what GNU does — so the memory cost below is
  not a cost relative to GNU, only relative to what we had.

**Against it.**

* *Memory.* `fs::read_dir` streams; this does not. A directory with ten million
  entries now costs a `DirEntry` each — on the order of a hundred bytes with
  the name — where before it cost one. That is the real price, and it is paid
  on directories nobody has, by a program that is about to `stat` every one of
  those entries anyway.
* *A `readdir` that fails part-way through now abandons the directory* instead
  of copying the entries already read and then reporting. This is a behaviour
  change, and it is *toward* GNU: `savedir` returns `NULL` on any error and
  `copy_dir` reports the single `cannot access` diagnostic, having copied
  nothing. The old streaming code copied a prefix and then complained, which
  no version of GNU does.
* *The order is not portable and the sort does not make it so.* Two filesystems
  can allocate inodes differently and both programs will then agree with each
  other while disagreeing with a third machine. That is acceptable because the
  claim is only ever "same as GNU here", never "same everywhere" — and it is
  the claim `cp-diff.sh` actually tests.

### The alternative that was rejected

**Sort by name.** Deterministic everywhere, easy to explain, and what a user
might expect. It is wrong for the only reason that matters: GNU does not do it,
so `cp -rv` would still differ — just differently, and now in a way that looks
deliberate enough that nobody would go back and check.

### Where it bites

`userspace/coreutils/src/bin/cp.rs` → `read_dir_fastread`, and its two tests
`the_directory_read_returns_every_entry_once` (both platforms) and
`the_directory_read_is_in_inode_order` (`#[cfg(unix)]`).
`scripts/cp-diff.sh` section 14 has four `-rv` cases over multi-entry
directories that go red without it, and the fixture comment above `mktree`
records that the fixture's creation order is now load-bearing for that reason.

**`rm` asks the same question and gets the opposite answer, correctly.** It has
had `-v` for some time, and `scripts/rm-diff.sh` already compares `-rv` over
multi-entry directories — green, with no sort anywhere. That is not luck: GNU's
`rm` walks with `fts_open` and a *null* comparison function, so it takes raw
`readdir` order, and so do we. Both unsorted is as good a match as both sorted;
what would break either program is one of each. So the rule is not "sort" but
"do what the program being matched does", and the two answers differ only
because upstream's two programs do.

### A measurement worth keeping, because it contradicts a comment we had written

On the ext4 filesystem the harnesses run on, `readdir` order for a three-entry
directory is *not* insertion order:

```
$ ls -f tree        $ ls -i tree
..                  674264 a.txt
a.txt               674266 link
link                674263 sub
sub
.
```

`dir_index` is on, so even a directory this small is hashed, and hash order is
a function of the *names*. `rm-diff.sh`'s fixture comment claimed the opposite
— "a directory this small is a linear one, whose readdir order is insertion
order" — and has been corrected. Its conclusion survives the correction and is
in fact stronger: hash order depends only on the names, so two case directories
holding the same names enumerate identically however they were built. What
would break that is a fixture whose two sides hold *different* names, which is
a thing no harness here does.
