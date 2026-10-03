## B-THE-KERNEL-XATTR-API-LOSES-TWO-THINGS-USERSPACE-NEEDS (lane B, 2026-08-31) — FIXED 2026-08-31

**In short.** Two defects in the kernel's extended-attribute path, both found
while giving `cp` xattr support, both in `kernel/**` and so filed rather than
fixed: `requests/b-a-a-missing-xattr-and-a-missing-file-are-the-same-error.md`.

**1. "No such attribute" and "no such file" are one error code.** `KernelError`
(`kernel/src/error.rs`, Filesystem band 500–599) has no variant for "the object
exists, the attribute does not", so `ext4/vfs_impl.rs:1527` and
`memfs.rs:122` both return `NotFound` after having successfully found the
inode. `posix/src/errno.rs` maps that to `ENOENT`; Linux says `ENODATA`.

Costs today, all live:

- `posix/src/xattr.rs:182–202` implements `XATTR_CREATE`/`XATTR_REPLACE` with a
  get-then-set probe, because it cannot get the flag semantics from one call.
  Two syscalls where Linux does one, with a race window between them in which
  the two outcomes the flags exist to forbid can both happen.
- That probe reads *any* negative return as "attribute absent", so
  `setxattr("/no/such/file", …, XATTR_REPLACE)` reports `ENODATA` — an answer
  about an attribute — when the truth is `ENOENT`.
- `cp --preserve=all` suppresses `ENOTSUP`/`ENODATA` and reports everything
  else, which is GNU's policy (`copy.c`'s `copy_attr`) and is now ours. An
  attribute removed by another process between `cp`'s `listxattr` and its
  `getxattr` is `ENODATA` on Linux and silent; here it arrives as `ENOENT` and
  prints `cp: getting attribute 'user.tag' of 'a': No such file or directory`
  about a file that is fine.

**2. An attribute name is typed as UTF-8, and one bad name fails the whole
inode.** `FileSystem::get_xattr(…, key: &str)`, `list_xattrs → Vec<String>`,
and `read_user_cstring` (`handlers.rs:139`) all require UTF-8. An xattr name on
Linux is an opaque NUL-terminated byte string. The damage is at
`ext4/driver.rs:3508`:

```rust
let name = core::str::from_utf8(name_bytes).map_err(|_| KernelError::IoError)?;
```

inside the loop over *every* attribute on the inode, so one non-UTF-8 name
makes `read_all_xattrs` return `EIO` and takes `get_xattr`, `list_xattrs` and
`set_xattr` down with it for that file — including for the attributes whose
names are ordinary. `cp -a` of such a file reports an I/O error on a healthy
filesystem, and the offending attribute cannot be read or removed.

This is CLAUDE.md self-review item 7, and it is the same defect this tree
already fixed for directory entries — the comment left at
`ext4/driver.rs:5081` records that lesson in almost these words. The xattr path
never got the same pass, and its failure mode is louder: the directory fix was
recovering from *skipping* one entry, this one fails the inode.

Realistic source: a filesystem written by Linux. Nothing here produces a
non-ASCII attribute name; `setfattr` on a Linux host will, and `rootfs.ext4` is
built on one.

**Lane B's side is already right** and needs no change when this lands:
`userspace/coreutils/src/fsattr.rs` passes and returns `&[u8]` throughout, and
`cp` quotes attribute names as bytes.

**Not a blocker.** `cp`'s xattr support shipped 2026-08-31 without either fix.
Defect 1's cases are races and unusual arguments; defect 2 needs a file this
tree cannot yet create. Filed now because the cost of 1 grows quietly with each
new caller that reads `ENOENT` from an xattr call, and because `--preserve=mode`
ACL support — which needs `removexattr` to answer "already absent" distinctly —
is the next thing due on `cp`.

**Fixed 2026-08-31.** Lane A fixed both defects and, unasked, the flags word
that would have made the probe unnecessary: `32f35d46b` (`NoAttribute = -514`,
plus xattr names typed `&[u8]` end to end — and a *second* UTF-8 site this
entry did not find, `parse_inline_xattrs`, which silently truncated the
attribute list rather than failing it), `0593342d9` and `8c8ba6acb` (`arg5`
bits 1–2 = `XATTR_CREATE`/`XATTR_REPLACE`, decided under the same `fs.lock()`
hold as the write). Lane B's side: `posix/src/errno.rs` maps `-514` to
`ENODATA`; `posix/src/xattr.rs`'s `do_setxattr` forwards the flags as bits 1–2
and the get-then-set probe is gone; `fsattr.rs`'s `remove_xattr` no longer
accepts `ENOENT` as "already absent", so a `removexattr` on a path that does
not exist is now reported instead of swallowed.

One deliberate difference from the request's table survives in userspace. The
kernel answers `EINVAL` for `XATTR_CREATE|XATTR_REPLACE`; Linux does not — it
passes the pair down and lets the filesystem answer `EEXIST` or `ENODATA` from
the actual state. `do_setxattr` therefore still resolves that one combination
itself, with a `SYS_FS_GET_XATTR` size query to choose which of the two
failures to name. That is not the old probe returning: the old one raced
between a *read* and a *write*, and this one never writes at all — both flags
set can never succeed, so the worst a race can cost is the wrong errno on a
call that was already doomed. Every other flag combination goes to the kernel
unexamined, and `setxattr_flags_valid` refuses the bits above 2 before they
can reach `arg5`, which is the masking lane A asked for.
