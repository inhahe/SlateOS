## 733. `O_PATH` is honoured for everything userspace can observe and left unimplemented for the part that needs the kernel, rather than being refused outright

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous) · **Lane:** B

**In short:** `O_PATH` is a flag that asks `open` for a descriptor naming a
*file* without actually opening it — useful for pinning a directory you will
then operate on with the `*at` calls, without asking for permission to read it.
We accepted the flag and then ignored it, so every operation a program tries on
such a descriptor quietly worked when Linux would have refused it. We now
refuse them, but we still get the descriptor by really opening the file. The
choice was between doing that half, doing nothing, and making the flag an
outright error; the half was chosen because it is the half a program can see,
and because the other two options each break something that works today.

### The problem

`open` stored the flag in the descriptor's status flags and nothing ever read
it. Concretely, on a descriptor opened `O_PATH`, Linux answers `EBADF` to
`read`, `write`, `pread`, `pwrite`, `readv`, `writev`, `preadv`, `pwritev`,
`lseek`, `ftruncate`, `fsync`, `fdatasync`, `flock`, `fchmod`, `fchown`,
`fallocate`, `posix_fadvise`, `ioctl`, a file-backed `mmap`, and four of
`fcntl`'s commands — and we answered all twenty as though the flag were not
there. The failure direction is *over-permissive*, which is why nothing
noticed: no program fails, things merely work that should not.

There is a second, smaller half that userspace cannot fix. `O_PATH` is also
supposed to let you open a file you may not read, and to let you name a symlink
without following it. Both require a kernel handle that has no open file
description behind it. Ours does not exist, so we obtain the descriptor by
opening the file for real.

### The options

| | *What changes:* |
|---|---|
| **A. Keep ignoring it** | Nothing. A program using `O_PATH` defensively gets none of the protection it asked for, silently. |
| **B. Refuse it — `open` returns `EOPNOTSUPP`** | Programs that pass `O_PATH` stop working, loudly, instead of working over-permissively. |
| **C. Honour the observable half** | Operations on the file report `EBADF` as Linux does. Obtaining the descriptor still requires read permission, and still cannot name a symlink. |

**C was chosen.**

### Why not B, which is what this file's own precedent seemed to argue for

The precedent is `O_TMPFILE`, which this libc refuses with `EOPNOTSUPP` rather
than ignoring, and the `known-issues.md` entry for `O_PATH` originally proposed
copying it. On re-examination the precedent does not transfer, for two reasons.

The first is that the two flags fail differently when ignored. `O_TMPFILE`
names a *directory* and asks for an unnamed file inside it; ignore the flag and
`open` succeeds on the directory itself, which is not a thing any caller
asked for and which subsequent writes will do damage through. `O_PATH` names
the file the caller actually named; ignore the flag and the caller gets a
working descriptor for the right file with more authority than it wanted.
Over-authority is a real bug, but it is not the same *kind* of bug, and the
"turn a silent divergence into a loud one" argument is only compelling when the
silent behaviour is actively wrong rather than merely lax.

The second is decisive: three uses of `O_PATH` already work end to end here.
It works as a `dirfd` for the whole `*at` family, because the fd table stores
the resolved path and the `*at` calls join against it. It works as an argument
to `fstat`, which Linux also permits. And it works as an argument to `fexecve`,
which resolves the descriptor through `fdtable::get_fd_path` and never touches
the file. `EOPNOTSUPP` would break all three in order to make the fourth loud —
trading three working behaviours for one diagnostic.

### Why C is not merely "the easy part"

The objection to a half-fix is that it produces a flag which is *partly* real,
and a program may reason from the working half to the broken half. That risk is
narrower than it sounds, because the two halves are separated by what a program
can test. A program that opens `O_PATH` and then reads gets the right answer.
A program that opens `O_PATH` on a file it cannot read gets an error *at the
open*, which is a failure it must already handle, not a wrong success. The
divergence is therefore "an open that should have succeeded did not" — the safe
direction — rather than "an operation that should have failed did not", which
is what C removes.

### The check goes in exactly one place, and that is not a stylistic choice

Linux does not repeat this test in twenty syscalls. `fdget()` — the generic
"turn a descriptor number into a file" helper — masks `FMODE_PATH` and returns
nothing, so the refusal is emitted at precisely the position of the closed-fd
`EBADF`. `fdget_raw()`, used by the handful of calls that legitimately accept
these descriptors, does not mask it. That single mechanism is what makes the
implementation mechanical: put the check beside each descriptor lookup and
every ordering question answers itself, because whatever outranks `EBADF` for a
*closed* descriptor outranks it here, and nothing else does.

That claim was checked rather than assumed, in both directions:
`ftruncate(path_fd, -1)` is `EINVAL` and so is `ftruncate(999closed, -1)`;
`lseek(path_fd, 0, <nonsense whence>)` is `EBADF` and so is
`lseek(999closed, 0, <nonsense whence>)`. Both pairs are pinned by tests.

### What it cost, which is the part worth recording

Placing the check at the descriptor lookup meant two functions had to *acquire*
a descriptor lookup at that position, because they had been validating their
arguments first. `lseek` reported `EINVAL` for a bad `whence` on a closed
descriptor where Linux reports `EBADF`, and `posix_fadvise` reported `EINVAL`
for a bad `advice` or a negative `len` on a closed descriptor where Linux
reports `EBADF`. Both were pre-existing bugs, invisible until this rule made
them contradictions; both are fixed, with the measured orderings in comments
and tests. `posix_fadvise`'s old tests had even encoded the wrong rule as a
comment — "Linux validates advice before touching the fd table; we do the
same" — which it does not.

A third measurement fell out of the same probe: `lseek(f, -5, SEEK_DATA)` is
`ENXIO` on both ext4 and tmpfs, not the `EINVAL` we returned, because a
negative start is reached by the same `offset < 0 || offset >= i_size` test
that reports a past-EOF start. Our kernel reports that condition as the generic
`InvalidArgument`, so `lseek` remaps it — precisely, since `fs/handle.rs`'s
`SeekFrom::Data`/`SeekFrom::Hole` arms are the only places those two syscalls
can produce it.
