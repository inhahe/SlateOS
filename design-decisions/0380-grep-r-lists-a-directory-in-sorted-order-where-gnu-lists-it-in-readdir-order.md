## §380 — `grep -r` lists a directory in sorted order, where GNU lists it in readdir order

**Date:** 2026-08-25
**Decided by:** Claude (autonomous)

**In short:** When you point `grep -r` at a folder, it has to pick an order to
search the files in. GNU searches them in whatever order the disk happens to
hand them back, which is not the same on two computers holding the same files —
so the output of the same command can come out shuffled differently on each.
Ours sorts the names instead, so the same tree always produces the same output.
The cost is that our output does not match GNU's line-for-line on a folder with
more than one file in it, even when every line in it is identical.

### What was decided

`Run::walk_entries` collects a directory's entries, sorts them by path, and then
visits them. GNU's `grep` visits them in the order `readdir(3)` returns, which
on ext4 with hashed directories is a hash order, on other filesystems is
insertion order, and on none of them is specified.

The difference shows up in `scripts/grep-diff.sh` as one case — `grep -r foo .`,
the only one whose expansion is wide enough to expose it — marked `!` with the
reason recorded inline.

### Why sorted

- **Reproducible.** Two machines with the same tree give the same bytes. That is
  the property a differential harness needs to exist at all, and it is the
  property a user needs to `diff` two runs.
- **Diffable and greppable by a human.** A sorted listing can be scanned for a
  missing entry. A hash-ordered one cannot.
- **`ls` already sorts, for exactly these reasons,** and a user who has learned
  that `ls` gives a stable order has no reason to expect `grep -r` to be the
  odd one out.
- **The order is not part of the interface anyone can rely on.** Nothing in
  POSIX or in GNU's documentation promises readdir order, and a script that
  depends on it is already broken across filesystems.

### Why readdir order was the alternative

- **Byte-for-byte fidelity with GNU.** It is the only way this harness case
  could pass rather than be marked, and "differs from GNU" is a cost we
  otherwise work hard to avoid.
- **Speed on a huge tree.** Sorting needs the whole directory in memory before
  the first file is searched. For a directory with a million entries that is a
  real allocation and a real pause before the first line of output. (Bounded:
  it is one directory's worth, not the whole tree — the walk itself streams, so
  the peak is the widest single directory, not the sum.)
- **Ordering by inode is faster to *read*,** which is why some tools deliberately
  visit in readdir order: it follows the on-disk layout.

The reproducibility argument wins because the pause is per-directory and
proportional to a directory people rarely have, while the shuffled output is
paid on every multi-file run by everyone. If a profile ever shows the sort
mattering, the fix is to sort lazily in chunks, not to abandon the order.
