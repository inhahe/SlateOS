## 724. `cp -r` refuses a copy into itself before it writes anything, where GNU discovers it mid-walk and leaves whatever it had already written

**Date:** 2026-08-30
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `cp -r tree tree` asks for a directory to be copied inside
itself, which cannot terminate, and both our `cp` and GNU's refuse it with the
same message and the same exit status. They differ in what is left on disk
afterwards. Ours leaves the tree exactly as it found it. GNU leaves a partial
copy — sometimes a complete one, sometimes an empty directory — because it
only notices the problem when the walk trips over the destination it made
earlier. We keep our behaviour and mark those cases in the differential
harness as differing on purpose, because GNU's leftovers are not a behaviour
anything could rely on: the same command on the same files produces different
leftovers depending on which inode number the kernel hands out.

### The two mechanisms

Ours is a check on the paths, run once, before any directory is created: the
destination is resolved and compared against the source, and if the
destination lies inside the source the copy is refused whole.

GNU's is a check on inode numbers, run continuously during the walk
(`copy.c`). When it creates the first destination directory for a command-line
argument it records that directory's `(device, inode)` pair
(`remember_copied`, `copy.c:2982`). Every directory the walk later descends
into is looked up in that table; a hit whose name is the same directory entry
produces

```
cp: cannot copy a directory, 'tree', into itself, 'tree/tree'
```

and breaks out of the loop (`copy.c:2701`, `copy_dir`'s `if
(local_copy_into_self) break;`). Everything copied before that moment stays.

### Why the leftovers are not reproducible

`copy_dir` snapshots the source directory with `savedir (…,
SAVEDIR_SORT_FASTREAD)`, which orders the entries **by inode number** — a
speed optimisation, since reading inodes in ascending order is close to
sequential on disk. The destination directory is created just before that
snapshot is taken, so it appears in it, and *where* it appears is decided by
the inode number the filesystem happened to allocate for it.

On a young filesystem a new directory gets a high inode, sorts last, and the
walk therefore copies the whole source before tripping. That is what makes the
behaviour look deterministic. It is not. Measured under GNU coreutils 9.4 on
ext4, with the same command, the same source contents and the same binary:

| Free-inode state before the copy | Destination's inode | Residue |
|---|---|---|
| fresh directory, 100 files | higher than every source entry | the complete 100-file copy |
| 1900 files created then deleted first | *lower* than every source entry | one empty directory |

The second row was produced by creating 2000 files, deleting the first 1900 to
free their inodes, and copying the remaining 100 — the new directory reused a
freed low inode (556010, against a surviving range of 821019–821118), sorted
first, and was tripped over immediately. Nothing about the request changed.

So the residue is a function of the filesystem's free-inode list. It is also a
function of the filesystem *type*: `SAVEDIR_SORT_FASTREAD` degrades to plain
readdir order where `struct dirent` carries no `d_ino`.

### The alternatives

- **Match GNU, residue and all.** *For:* the harness's standard is "no
  observable difference", and the filesystem is observable. *Against:* there
  is no fixed thing to match. Any test asserting a particular residue would
  pass or fail on the state of the free-inode list, which is exactly the flake
  the harness exists to prevent — and we would be reproducing an artefact of
  an optimisation (`SAVEDIR_SORT_FASTREAD`) rather than a decision.
- **Refuse early, as we do.** *For:* same diagnostic, same exit status, and
  the one difference is that a command which failed changed nothing. A user
  who mistypes `cp -r tree tree` does not then have to work out which of the
  files under `tree/tree` are real. *Against:* it is a genuine behavioural
  difference from GNU, and a script that somehow depended on the partial copy
  would not be served — though such a script would already be broken on GNU,
  per the table above.
- **Refuse early *and* clean up after GNU's fashion** — copy, then remove what
  was written. Rejected: it does strictly more filesystem work to reach the
  same end state, and a failure during the cleanup would leave a mess worse
  than either alternative.

### Where this bites

Six cases in `scripts/cp-diff.sh` are marked `xfail_case` with this reason:
`-r tree tree`, `-r tree tree/sub`, `-r . dst`, `-r ./ dst`, `-r tree/.. dst`
and `-r tree dir dir`. The stderr and exit status of all six agree exactly;
only the directory snapshot differs. If our resolution ever stops refusing one
of them, the case turns into an `XPASS` and says so.

The other four spellings in that section (`-r tree .`, `-r tree ./`, `-r
tree/. dst`, `-r tree/sub/.. dst`) agree completely, because there GNU refuses
before writing too — the destination already exists as a directory, so no
first directory is created and the trip happens on the first entry.
