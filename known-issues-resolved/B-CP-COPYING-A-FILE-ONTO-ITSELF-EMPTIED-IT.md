## B-CP-COPYING-A-FILE-ONTO-ITSELF-EMPTIED-IT (lane B, 2026-08-30) — FIXED

**In short:** `cp a a` deleted the contents of `a`. It printed nothing and
exited 0, so nothing anywhere reported that a file had just been destroyed.
The same happened for `cp a ./a`, `cp a dir/../a`, `cp a <hard-link-to-a>`,
`cp a <symlink-to-a>` and `cp -r a .`. GNU refuses all six with
`cp: 'a' and 'a' are the same file`. Fixed the same day it was found, in
`userspace/coreutils/src/bin/cp.rs`.

**Why it destroyed data.** The copy went through `std::fs::copy`, which opens
the destination with `O_TRUNC` before it reads the source. Handed one file
under two names, it truncated that file to zero length and then copied the
resulting nothing back over itself. `fs::copy` returned `Ok(0)`, so `cp`
reported success.

**Why no string comparison would have caught it.** Only the first of the six
spellings has two equal operands. The rest reach one inode by two names — a
`.` or `..` in the middle of a path, a hard link, a symlink, or a directory
destination that resolves to the source's own parent. GNU does not attempt a
textual comparison: `same_file_ok` in `src/copy.c` compares the two `stat`
results' device and inode. `cp.rs` now does the same, in `is_same_file`, at
the same point in the same order — after the `-r not specified` refusal and
before anything touches the destination.

**The order of the checks was wrong too, and is fixed with it.** `cp.rs` asked
"is the source a directory and was `-r` given?" *after* working out where the
source would land, so `cp tree/..` (no `-r`) complained about the destination
rather than about the missing `-r`. GNU asks about the source first, because
the refusal is a fact about the source alone.

**How it was found, and the general point.** By `scripts/cp-diff.sh` on its
first run — a new differential harness, written before any new feature, purely
to certify what `cp` already did. It found this in the first thirty seconds.

The file it found it in had *already been rewritten once*, in a change whose
commit message lists six bugs found by reading the code it replaced. That
rewrite introduced this one: it swapped a hand-rolled byte-copy loop for
`fs::copy` and never asked what `fs::copy` does when handed one file twice.
So the lesson is not "read more carefully" —

> **A rewrite that fixes six defects found by reading is exactly the change
> most likely to introduce a seventh that reading will not find.** Reading
> finds the bugs you thought to look for. The differential harness is what
> finds the others, and it is worth writing *before* the feature work it is
> meant to protect, not after.

This is the second finding in a week that a measurement produced and no amount
of re-reading would have — see the `nice` entry above, where the defect was a
`#if` arm that is dead on glibc, and
`B-QUOTING-NO-MEASURED-ROW-HELD-BOTH-A-SLASH-AND-A-QUOTE`.

**Certified by** `scripts/cp-diff.sh` sections 4 and 6, and by three unit
tests in `cp.rs` — one of which asserts the *bytes* of the source afterwards
rather than the diagnostic, because the defect reported success and said
nothing, and a test that checked only the message would have passed against it.
