## §663 — `mkdir`'s mode is ten bits (`0o1777`) on both routes: sticky in, setuid/setgid out

**Date:** 2026-09-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** When a program creates a directory it passes a number saying who
may read, write and enter it. Twelve bits of that number mean something on a
file; only ten of them mean anything on a *new directory*. We were accepting
nine on one route and twelve on the other, and — as it turned out — throwing
away all but nine on both, one layer further down, where nobody was looking.
All four places now agree on ten: the nine ordinary permission bits, plus the
"sticky" bit that makes a shared directory like `/tmp` safe by stopping people
deleting each other's files. The two bits we now refuse (`setuid`/`setgid`)
were never settable this way on Linux either, and one of them silently decides
who owns files created in the directory afterwards.

### What was actually there

Four masks sat on one operation, and no two of them agreed:

| Layer | path route (660) | pinned route (666) |
|---|---|---|
| `posix`'s `apply_umask_mkdir` | `0o777` | `0o777` |
| syscall handler | `0o7777` (since §639) | `0o777` |
| `Vfs::mkdir_mode` / `mkdir_at_pinned` | `0o777` | `0o777` |

**The middle row never mattered.** §639 Decision 3 widened
`sys_fs_mkdir_mode` to `0o7777` on 2026-08-30 on the argument that "the mask
was never a limit of the filesystem, only of this line." That argument was
correct about `open`, where the widening did reach the disk, and wrong about
`mkdir`, where it was *not this line* — `Vfs::mkdir_mode` masked the mode to
`0o777` again before stamping it, so `mkdir(path, 0o1777)` produced exactly
the `0o777` directory it had produced the day before. The widening was dead
for two days and no test could see it, because every mode any test asserted
lay inside `0o777`.

This is the same shape lane B described in
`requests/b-a-666-669-are-wired-two-answers-and-one-bug-that-was-mine.md` §4,
where `posix`'s `apply_umask` was undoing the same §639 widening for `open`:
**a widening recorded in one layer and quietly re-narrowed in another produces
no error on either side**, because "the bit is missing" and "the bit was never
requested" produce identical directories. Theirs crossed a lane boundary;
this one did not cross anything — it was two functions in one crate, both
written by this lane, three weeks apart. The lane boundary was never what made
it invisible. The absence of a test that asserts a bit *outside* the old mask
was.

### Decision 1 — sticky is accepted

`S_ISVTX` is the one bit where creating it in a single step buys something
real. The two-step alternative — `mkdir(0o777)` then `chmod(0o1777)` — leaves
a window in which the directory is world-writable and *not yet* sticky, and in
that window any user may delete any other user's files in it. Every other mode
bit's two-step is merely untidy; this one's is a hole.

Linux honours it here for the same reason — `mkdir(2)` VERSIONS: "Under Linux,
apart from the permission bits, the `S_ISVTX` mode bit is also honored" — and
`vfs_mkdir` masks to precisely `S_IRWXUGO|S_ISVTX`.

**Against:** GNU coreutils never passes special bits through `mkdir()` even on
Linux where they would be honoured (`lib/mkdir-p.c:117-130` deliberately
creates with restricted permissions first and applies special bits after, so
that unauthorized users "cannot nip in before the directory is ready"). So
`mkdir -m 1777` is two syscalls on GNU/Linux too, and widening buys *that*
caller nothing. Lane B, who ships our `mkdir`, said plainly they have no
caller today.

**Why it went in anyway:** the width is ABI, and ABI is cheap to widen now and
expensive to widen later. A native caller — anything using the `SYS_FS_*`
family directly rather than through libc, which is the whole point of that
family existing — has no `mkdir-p.c` to imitate and no reason to write the
two-step. The bit costs one character in a mask and closes a race for whoever
does want it.

### Decision 2 — setuid/setgid are refused, reversing §639 for `mkdir` only

This is a *narrowing* of what 660 accepted since 2026-08-30, and it is a
partial reversal of §639 Decision 3, which widened `SYS_FS_MKDIR_MODE` to
`0o7777` "to match" `SYS_FS_OPENAT2`. The match was the mistake:

- **The specification splits it here, not merely the implementation.** This is
  lane B's citation and it is the load-bearing one, because it puts the
  asymmetry in the *interface* rather than in one kernel's choices. `mkdir(2)`
  DESCRIPTION: "in the absence of a default ACL, the mode of the created
  directory is `(mode & ~umask & 0777)`." That `& 0777` is stated for `mkdir`
  and is **absent** from `open(2)`, which says only `(mode & ~umask)`. So
  `open` taking twelve while `mkdir` takes fewer is not an inconsistency we
  are tolerating — it is the shape the interface actually has, and §639's
  "to match" was matching two calls that the standard does not match.
- **Linux splits it in the same place**, and its one extension over the spec
  is exactly the bit Decision 1 keeps: `vfs_create` keeps `S_IALLUGO` (all
  twelve); `vfs_mkdir` keeps `S_IRWXUGO|S_ISVTX` (the spec's nine, plus
  sticky). The reason is in the same man page — a new directory's setgid bit
  is *inherited from its parent*, never taken from the caller's mode word:
  "If the parent directory has the set-group-ID bit set, then so will the
  newly created directory."
- **So accepting it offers an authority `mkdir(2)` does not have**, in the one
  bit that decides who owns files created in that directory later. A caller
  could produce a directory Linux could not.
- **And §639's own safety argument runs the other way here.** §639 justified
  storing an unenforced `setuid` on a file by noting the failure is
  "less privilege than the metadata claims", which is fail-closed. That
  reasoning does not transfer: we do not implement setgid-directory
  inheritance at all, so a stored setgid bit on a directory is metadata
  asserting a semantic the kernel never performs, and a program that reads it
  to decide "the group will be inherited, so I need not `chgrp`" is wrong in
  whichever direction the group topology happens to point. An unenforced bit
  is only safe when the thing it would have granted is *privilege*; this one
  would have granted an *expectation*.

**What it costs:** a caller wanting a setgid directory does it in two calls,
via `SYS_FS_FCHMODAT_PINNED` (665), which still takes all twelve. That is the
same shape GNU already uses for special bits, and the request is explicit and
separately auditable where a mode-word bit is neither.

### Decision 3 — the same mask on all four sites, in one change

660 and 666 mask identically, and so do `Vfs::mkdir_mode` and
`Vfs::mkdir_at_pinned`. One operation with two masks depending on which route
ran is worse than either mask consistently applied — the argument that settled
`link`'s error code in §662's week, applied to a width instead of a code.

Lane B asked for the same thing and for the same reason, and added the part
that makes it a *cross-lane* constraint: `apply_umask_mkdir` masks to `0o777`
on their side, so until they move it to `0o1777` the widening is once again
visible only to native callers. That is not a bug — it is the trap in Decision
1's own history repeating one layer up — so the request went out in the same
exchange rather than after it. The narrowing half (setuid/setgid) needs
nothing from them: their mask already drops those bits.

### The test that would have caught it

`vfs_selftest` now creates `0o1755` on **both** routes and asserts `0o1755`
reads back, and creates `0o2755` on the pinned route and asserts `0o755`. The
first pair is the one that matters: every previous mkdir assertion used a mode
inside `0o777`, which is precisely why a `& 0o777` two layers down survived
two days of green tests. **A mask can only be tested by a value outside it.**

### The same mask, found three more times by going and looking

Having established that the bug is "a nine-bit mask under a twelve-bit
contract, invisible because nothing tests a tenth bit", the obvious next move
was to grep for the other nine-bit masks rather than assume this was the only
one. Three of the read-only backends had it, in `metadata()`:

```rust
// btrfs, zfs
let permissions = u16::try_from(inode.mode & 0o777).unwrap_or(0o444) & 0o555;
// f2fs
let permissions = (inode.mode & 0o777) & 0o555;
```

All three carried a comment saying the mode "is reported **as-is** — with the
write bits masked off", which is an accurate description of `& 0o555` and a
false description of the `& 0o777` in front of it. The `& 0o555` is justified
and stays: the mount refuses every write, so advertising write permission is a
lie userspace acts on. But **that argument reaches the three write bits and
stops.** A read-only mount does not make a setuid binary not-setuid; it makes
it unmodifiable. Sticky on a read-only directory is equally true and equally
inert.

Meanwhile `FileMeta::permissions` documents itself as twelve bits — "setuid
setgid sticky rwxrwxrwx" — and ext4 (`i_mode & 0o7777`) and iso9660
(`m & 0o7777`) deliver twelve. So the same file, with the same bits on disk,
reported a different mode depending on which filesystem driver read it, and
`cp -a` or `tar` pulling data *off* a btrfs/zfs/f2fs mount silently dropped
setuid, setgid and sticky from everything it copied. Fixed to `& 0o7555` —
twelve bits minus the three write bits, which is what the comment always
claimed.

The generalisable part is not the mask. It is that **a documented contract
with more than one implementation needs a test that reads the contract, not
one per implementation** — nothing here was wrong in a way any single
backend's own tests could see, because each backend was internally consistent.
Logged in `known-issues.md` as `A-NO-CROSS-BACKEND-METADATA-CONFORMANCE-TEST`,
because the fix above closes the three instances and not the hole that let
them diverge.
