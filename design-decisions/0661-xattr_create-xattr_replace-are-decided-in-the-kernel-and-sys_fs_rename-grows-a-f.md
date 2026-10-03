## §661 — `XATTR_CREATE`/`XATTR_REPLACE` are decided in the kernel, and `SYS_FS_RENAME` grows a flags word

**Date:** 2026-08-31 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** Two of our syscalls made userspace fake a guarantee it could not
actually provide. "Create this attribute, but fail if it already exists" was
being spelled as two separate system calls — look, then write — and anything
can happen in between, so the "fail if it exists" part was a wish rather than a
promise. The same held for "rename this file, but fail if the new name is
taken". Both now take a flag telling the kernel what was wanted, so the check
and the action happen together and cannot be split apart.

### The problem

`posix/src/xattr.rs` implemented `XATTR_CREATE` and `XATTR_REPLACE` by issuing
`SYS_FS_GET_XATTR` and reading "did that succeed?" as "does the attribute
exist?", then issuing `SYS_FS_SET_XATTR` regardless. Two syscalls are two
acquisitions of the filesystem lock, with a window between them: a writer
arriving in that window turns `XATTR_CREATE` into an overwrite and
`XATTR_REPLACE` into a create — the two outcomes the flags exist to forbid.

The probe was also wrong in a way that has nothing to do with timing. It read
*every* negative return as "the attribute is not there", so
`setxattr("/no/such/file", …, XATTR_REPLACE)` answered `ENODATA` — a statement
about an attribute — when the truth was `ENOENT`, a statement about the file.
It could not do better while the kernel spent one code on both; §660 is what
makes the distinction available, and this entry is what consumes it.

`SYS_FS_RENAME` had the same shape one level up: `Vfs::rename_noreplace` and
`Vfs::rename_exchange` already existed and were already reachable through the
Linux-ABI `renameat2`, but the native syscall ignored `arg4` entirely, so a
native caller had no way to ask for either.

### The decision

Both flags words are decoded in the kernel, and in both cases the check runs
on the same side of the lock as the write.

For xattrs, `Vfs::set_xattr_with` takes an `XattrSetMode` and holds **one**
`fs.lock()` guard across the probe and the set. No filesystem changed: the
atomicity comes from the guard, not from new trait methods, because the mount
lock is what the two-syscall version was dropping between its halves.

Only `NoAttribute` is read as absence. Every other error propagates —
`NotFound` above all — which is the correction to the probe's second bug, not
merely a reimplementation of its first.

For rename, `arg4` decodes via `RenameMode::from_flags` using Linux's bit
values (`1` = `RENAME_NOREPLACE`, `2` = `RENAME_EXCHANGE`), so libc's constants
pass through unchanged.

### Unknown bits are refused, not ignored

Both decoders reject any bit they do not define, and both reject the two
"do these at once" combinations that no state can satisfy. This is Linux's
behaviour, and the reason is worth stating: a kernel that ignores a flag it
does not understand performs the *default* action and reports success. A
caller that asked to be refused an overwrite would get the overwrite and be
told it worked. Refusing is loud, recoverable, and lets a flag be added later
without an older kernel silently doing the wrong thing.

### The ordering hazard, and why it is already safe

`arg4` travels in `r8`, which nothing zeroes. The day the kernel starts
*reading* it, a libc still issuing `syscall4` would hand over whatever the last
call left in that register — turning ordinary renames into `EINVAL` or, worse,
exchanges. Lane B landed `posix: pass an explicit flags word to SYS_FS_RENAME`
(`e8fec2292`) first, and `SYS_FS_RENAME` has exactly one call site in the tree,
so the register is now written explicitly at every call. That commit was
verified present on `origin/main` before this one was written; the kernel half
must not be merged ahead of it.

### CORRECTION 2026-08-31 — "already safe" was checked one layer too high

The paragraph above establishes that the *source* of the caller was fixed
before the kernel started reading `arg4`. That is necessary. It is not
sufficient, and the reason is a layer the paragraph never mentions: **between
lane B's source and the instruction that actually issues the syscall sits a
prebuilt artifact.** The boot-test fixtures do not compile `posix` — they link
`toolchain/sysroot/lib/libc.a`, and fastpy resolves that sysroot to a *sibling*
`os` checkout rather than to the worktree being tested
(`fastpy/compiler/toolchain.py:160-186`). All three lanes were linking an
11-day-old `libc.a` for exactly this reason.

So the check that was run — "is `e8fec2292` on `origin/main`?" — answers a
question about a file. The question that matters is whether the `libc.a` the
test binary links was *built from* that file. A `libc.a` predating `e8fec2292`
still issues `syscall4`, and the register hazard is live in the running system
while the source ordering looks impeccable.

Nothing here was actually broken: `libc.a` is rebuilt often enough, and the
observed staleness window did not straddle `e8fec2292`. But the argument was
unsound, and an unsound argument that happens to reach a true conclusion is the
kind that gets reused on a case where it does not. The general form:

> Ordering a source change before a consumer change proves nothing about
> execution order when a *build artifact* sits between them. Verify the
> artifact, not the commit.

The concrete follow-up is a boot-test gate that mirrors fastpy's sysroot
resolution, hashes the resolved `libc.a`, and fails when its contents differ
from `toolchain/sysroot/lib/libc.a` in the worktree under test — so "the tree I
tested is the tree I built" stops being an assumption.

### Testing

`fs::handle` gains a mode test that pins what the probe got *wrong* rather than
what it got racy — a single-threaded test cannot observe the race, but it can
observe which error each refusal spends, that a refused `CREATE` leaves the old
value intact, that a refused `REPLACE` does not create, and that `REPLACE`
against a missing path is `NotFound` and not `NoAttribute`.

The rename decode is tested as a pure function rather than through the handler,
because the rest of that handler reads user pointers and cannot run from kernel
space. That is a deliberate split: the decode is the part where an error would
be silent, so it is the part worth isolating to make testable.
