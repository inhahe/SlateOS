## 740. `readdir`'s `d_ino` is whatever the kernel put in the record, including `0` — libc does not manufacture an inode number for a filesystem that has none

**Date:** 2026-09-01
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** Every directory listing carries, for each file, a number that is
supposed to identify that file uniquely on its disk — the "inode number". Tools
use it to notice that two names are the same file: that is how `tar` and `rsync`
recreate hard links instead of copying the data twice, and how `find -samefile`
works. Some filesystems genuinely have no such number (the ones that are not
really disks — `/proc`, `/dev`, a CD-ROM), and for those our kernel reports `0`.
The question was what libc should hand to a program in that case: the honest
`0`, which means "unknown", or an invented number that at least looks unique.
We hand over the `0`.

### What this replaced

Until 2026-09-01 the value in `d_ino` was not an inode number at all: directory
streams listed via `SYS_FS_LIST_DIR` (603), whose record has no room for one, so
`readdir` reported *the entry's position in the listing* — 0, 1, 2, 3 — and the
raw `getdents64` did the same. That is never the file's real inode, and it is
worse than `0` in a specific way: it is plausible. `0` makes a caller say "I
don't know"; `4` makes it say "I know, and it's 4", and then the third and fourth
entries of any directory look like hard links to each other. Every same-file test
in userspace was quietly wrong rather than visibly unavailable.

`SYS_FS_GETDENTS_PINNED` (664) carries the same value `SYS_FS_GET_META` reports
as `st_ino`, so the choice below only exists now that there is a real number to
pass through.

### The decision

Pass the kernel's value through untouched, `0` included.

*What changes:* on `/proc`, `/dev`, `/sys` and iso9660, `ls -i` prints `0` and
`tar` treats every file as unlinked, rather than printing distinct-looking
numbers that mean nothing.

### The alternative, and who is doing it

Lane A's own `getdents64` (in the kernel-side compatibility path) substitutes an
FNV hash of the entry's name when the record's inode is `0`, so that callers see
*something* distinct per entry. Lane A logged the divergence itself
(`A-GETDENTS64-D-INO-DISAGREES-WITH-ST-INO-ON-PSEUDO-FILESYSTEMS` in
known-issues.md) and, in `requests/a-b-664s-record-now-has-an-inode-and-so-does-647.md`,
said plainly which side it would rather we took:

> Do not synthesise a replacement for a `0` you receive. […] If your loop does
> not treat `0` as deleted, passing the kernel's `0` straight through is strictly
> more honest than anything either of us can invent, and I would prefer that.
> Just don't reach for `pos`.

**For synthesising (lane A's current behaviour):** `ls -i` shows distinct
numbers; naive code that uses `d_ino` as a cheap per-entry key in a set keeps
working; nothing appears "broken" to a casual look.

**For passing `0` through (ours):**

- A synthesised inode **disagrees with `stat`**. `st_ino` for the same file comes
  from `SYS_FS_GET_META`, which reports the real `0`. So `find -inum $(stat -c
  %i f)` finds nothing, and any program that cross-checks the two sees a file
  whose identity changes depending on which call it asked. That is a harder bug
  to find than an absent number.
- It is **not stable across a rename** (the hash is of the name) and **not unique
  across directories**, so it does not actually deliver the property the caller
  wanted; it only delivers the appearance of it.
- `0` is what these filesystems mean. POSIX does not promise `d_ino` is
  meaningful on every filesystem, and the well-behaved callers — GNU `tar`,
  `rsync`, `du` — already skip hard-link detection when the inode is `0`,
  because Linux's own `/proc` and overlayfs corner cases taught them to.
- A libc that invents data is the wrong place to put a workaround. If the answer
  ever needs improving, it should improve in the kernel, where the filesystem is
  known, and then both routes get it at once.

The cost we accept: on pseudo-filesystems, tools that *do* rely on `d_ino`
without a `0` check see every entry as identical rather than as distinct. This
is the ordinary shape of "unknown" and is what those tools would see on Linux
against a filesystem that reports `0`.

### Reversal

One line in `fill_dirent64_batch` and one in `readdir`. If it ever turns out
that a real workload breaks on `0` where it would have survived a fake number,
the substitution belongs in the kernel's record — not in two libc call sites
that would then have to agree with each other forever.
