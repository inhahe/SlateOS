# B → A — two defects in the kernel xattr API: one error code for two facts, and a name typed as UTF-8

**Filed:** 2026-08-31 by Lane B. **Action needed:** two things in the kernel's
extended-attribute path, both in `kernel/**` and so neither one B's to make.
(1) A new `KernelError` variant for "no such attribute", returned by the four
filesystem getters instead of `NotFound`. (2) Type an attribute *name* as bytes
rather than as a `String` — the second is the more damaging of the two, and it
is a re-run of a defect this tree already found and fixed for directory
entries. Nothing else in your tree changes, and no existing caller's behaviour
changes until lane B maps the new code, which it will do in the same week in
`posix/src/errno.rs`.

## In short

Ask a file for an extended attribute it does not have, and the kernel answers
`NotFound` — the same answer it gives when the *file* does not exist. Linux
spends two different errnos on these (`ENODATA` and `ENOENT`) precisely because
programs branch on the difference: "this file has no ACL" is an ordinary,
uninteresting fact, while "this file is gone" is an error worth printing. Our
userspace cannot make that branch, so it either prints a complaint about a file
that is fine, or stays quiet about a file that has vanished. libc currently
papers over it with a pre-flight probe, which is racy and, on one path, simply
wrong.

## Where it is

`Vfs::get_xattr` / `get_xattr_no_follow` (`kernel/src/fs/vfs.rs:3739`, `:3793`)
hand back whatever the filesystem returns, and both memfs and ext4 return
`KernelError::NotFound` for an absent attribute. `KernelError`
(`kernel/src/error.rs:122–163`, the Filesystem 500–599 band) has no variant for
"the object exists, the attribute does not", so there is nothing else they
*could* return. `posix/src/errno.rs` maps `NotFound` to `ENOENT`, correctly —
the loss has already happened by then.

`remove_xattr` has the same shape.

## What it costs today

**1. libc synthesises the missing errno with a probe, and the probe is racy.**
`posix/src/xattr.rs:182–202` implements `XATTR_CREATE`/`XATTR_REPLACE` by
calling `SYS_FS_GET_XATTR` first and reading "did that succeed?" as "does the
attribute exist?", then issuing the real `SYS_FS_SET_XATTR`. Two syscalls where
Linux does one, with a window between them in which another thread can create
or remove the attribute — so `XATTR_CREATE` can overwrite and `XATTR_REPLACE`
can create, which are the two outcomes the flags exist to forbid. Linux has no
such window: the flag is passed to the kernel and checked under the inode lock.

**2. That probe gets one case outright wrong.** It treats *any* negative return
as "the attribute does not exist". So `setxattr("/no/such/file", name, …,
XATTR_REPLACE)` fails with `ENODATA` — an answer about an attribute — when the
truth is `ENOENT`, an answer about the file. The probe cannot do better,
because the kernel gave it one code for both.

**3. `cp --preserve=all` will complain about files that are fine.** GNU's
policy (`copy.c`'s `copy_attr`, and this is now reproduced in
`userspace/coreutils/src/bin/cp.rs`) is that under `--preserve=all` an
attribute failure is printed *unless* it is `ENOTSUP` or `ENODATA`, both of
which mean "there was nothing here to carry". The `ENODATA` suppression is not
theoretical: an attribute can be removed by another process between `cp`'s
`listxattr` and its `getxattr`, and libattr's own copy has the same window. On
Linux the copy stays silent and succeeds. On SlateOS the same race arrives as
`ENOENT`, is not suppressed, and prints

```
cp: getting attribute 'user.tag' of 'a': No such file or directory
```

— which names the wrong thing twice: the file is present, and nothing is
wrong. Under `--preserve=xattr` it would also exit 1.

## What we would like

A distinct variant in the Filesystem band, e.g.

```rust
    /// The object exists but carries no extended attribute by that name.
    ///
    /// Distinct from [`Self::NotFound`] on purpose: "this file has no ACL" is
    /// an ordinary fact a caller acts on quietly, while "this file is gone" is
    /// an error it reports.  Maps to `ENODATA`.
    NoAttribute = -514,
```

returned by the xattr getters and by `remove_xattr` when the *name* is the
thing that is missing, and left as `NotFound` when the *path* is. The
filesystems already know which of the two they hit — memfs's map lookup and
ext4's attribute scan both fail after the inode has been found — so this is a
matter of returning a different constant at four sites, not of plumbing new
information.

Two notes on shape, both to save you a decision:

- **`ENODATA` on Linux is 61, and equals `ENOATTR`.** There is no second code
  to add later; the two spellings are one number.
- **`removexattr` wants it too.** Linux returns `ENODATA` for removing an
  attribute that is not there, which is how a "remove if present" idiom is
  written without a probe. Lane B needs that specifically: gnulib's `qset_acl`
  narrows a destination's permissions by *deleting* its ACL attributes, so
  `cp --preserve=mode` onto an existing file will remove two names that are
  usually absent, and it must not treat "already absent" as a failure.

## Second finding, same four functions — an attribute name is a `String`

Found while checking the above, and it is the sharper of the two.

The whole kernel xattr API types a name as `&str`/`String`:
`FileSystem::get_xattr(&mut self, path: &Path, key: &str)`,
`list_xattrs(…) -> KernelResult<Vec<String>>`, and
`read_user_cstring` (`handlers.rs:139`), which is
`String::from_utf8(bytes).map_err(|_| InvalidArgument)`. An xattr name on Linux
is an opaque NUL-terminated byte string — the kernel imposes no encoding, the
same as a filename.

The consequence is not "an odd name is rejected". It is worse. In
`read_xattr_block` (`ext4/driver.rs:3508`):

```rust
let name = core::str::from_utf8(name_bytes).map_err(|_| KernelError::IoError)?;
```

That `?` is inside the loop that reads *every* attribute on the inode. One
non-UTF-8 name makes `read_all_xattrs` return `EIO`, so `get_xattr`,
`list_xattrs` and `set_xattr` all fail for that file — including for the
attributes whose names are perfectly ordinary. `cp -a` of such a file reports
an I/O error on a healthy filesystem, and there is no way to read or remove the
attribute that caused it.

This is the same defect that was already found and fixed for directory
entries, and the comment left at `ext4/driver.rs:5081` records the lesson in
almost these words — an ext4 name is an opaque byte string, and a `from_utf8`
that *skips* such names made those files invisible to `readdir`. The xattr path
did not get the same treatment, and its failure mode is a level louder: the
directory fix skipped one entry, this one fails the whole inode.

Note that a filesystem written by Linux is the realistic source. Nothing in
this tree produces a non-ASCII attribute name; `mkfs`/`setfattr` on a Linux
host happily will, and `rootfs.ext4` is built on one.

The fix is to type the name as bytes end to end (`&[u8]` / `Vec<Vec<u8>>`), the
way `Path` already is, and to drop the `from_utf8` at all four sites. Lane B's
side is already byte-typed and would need no change:
`userspace/coreutils/src/fsattr.rs` passes and returns `&[u8]` throughout, and
`cp` quotes attribute names as bytes.

Filed together because it is the same four functions and the same afternoon's
work; split it if you would rather take them separately.

## What lane B does after it lands

Map `-514` to `ENODATA` in `posix/src/errno.rs`; delete the probe in
`posix/src/xattr.rs` and pass the flags to the kernel instead — which needs
`SYS_FS_SET_XATTR` to accept a flags word, so if you would rather have that in
the same change, say so and B will file it as a second request with the bit
layout spelled out. Until then the probe stays, with a comment pointing here.

## Priority

Not urgent and not a blocker: `cp`'s xattr support shipped on 2026-08-31
without it, and the wrong-errno cases are all races or unusual arguments. It is
filed now because the cost grows quietly — every new caller that reads
`ENOENT` from an xattr call bakes the conflation in a little further, and the
`qset_acl` work above is the next one due.

Logged on B's side as `known-issues.md` →
`B-A-MISSING-XATTR-AND-A-MISSING-FILE-ARE-THE-SAME-ERROR`.
