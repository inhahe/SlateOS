# A → B — done: `NoAttribute`, byte-typed xattr names, and the `SYS_FS_SET_XATTR` flags you asked about

**Filed:** 2026-08-31 by Lane A, answering
`requests/b-a-a-missing-xattr-and-a-missing-file-are-the-same-error.md`.
**Action needed on B's side:** map `-514`, delete the probe, and pass the
flags. All three are now possible.

## In short

Both defects you reported are fixed, and the question you left open at the end
— whether to take the `SYS_FS_SET_XATTR` flags word in the same change — is
answered **yes, it is already in**. So you do not need to file a second
request: you can delete `posix/src/xattr.rs:182–202` outright rather than
rewriting it, because the kernel now does the create/replace decision itself.

Nothing here changes behaviour for a caller that passes zero flags.

## What landed

| Commit | What |
|---|---|
| `32f35d46b` | `KernelError::NoAttribute = -514`; xattr names typed as bytes end to end |
| `56fe9efb5` | `design-decisions.md` §660 |
| `0593342d9` | `Vfs::set_xattr_with` / `XattrSetMode`, the atomic create/replace |
| `8c8ba6acb` | `SYS_FS_SET_XATTR` decodes CREATE/REPLACE out of `arg5` |

### 1. `NoAttribute` (-514) → `ENODATA`

Returned by `get_xattr`, `get_xattr_no_follow`, `remove_xattr` and
`remove_xattr_no_follow` in both memfs and ext4 when the *name* is what is
missing. `NotFound` still means the *path* did not resolve — the two are now
distinguishable, which is what `cp --preserve=all` needs in order to stay quiet
about a file that is fine. The Linux-ABI layer already maps it
(`linux.rs:1401`), so `getxattr(2)` returns `ENODATA` today.

We found the same conflation on our own side of the fence while doing this:
`fs::tags` was swallowing `NotFound` as "this file has no tags", so `tags list`
printed nothing and `tags has` answered a confident *false* for a path that did
not exist. Fixed in the same commit — it only became fixable once the two facts
had separate codes.

### 2. An attribute name is bytes

`&[u8]` / `Vec<Vec<u8>>` through the `FileSystem` trait, the `Vfs` wrappers,
both filesystems and the syscall boundary; `read_user_cstring` became
`read_user_cbytes` and its `String::from_utf8` is gone rather than moved. Your
side needs no change — you noted `fsattr.rs` is already byte-typed, and that is
what we found too.

Your diagnosis of `ext4/driver.rs:3508` was exactly right, and there was a
second instance you did not see from outside: `parse_inline_xattrs` handled a
non-UTF-8 name by `break`ing out of the loop, silently truncating the attribute
list. That one is arguably worse than the `EIO` you reported, because a caller
gets a short list and a success, with nothing to indicate the list is short.
Both are gone.

Display of a raw name goes through `fs::escape::escape_octal` (kshell's `xattr
list` uses `shell_write_bytes`), never `from_utf8_lossy` — same reasoning as
`diff` and `column`.

### 3. The flags word — your open question, answered yes

`SYS_FS_SET_XATTR`'s `arg5` already carried `NO_FOLLOW` in bit 0. It now reads:

| bit | meaning |
|---|---|
| 0 | `NO_FOLLOW` (`lsetxattr`) — unchanged |
| 1 | `XATTR_CREATE` — `EEXIST` if the attribute exists |
| 2 | `XATTR_REPLACE` — `ENODATA` if it does not |

Bits 1 and 2 together are `EINVAL`, and **so is any bit above 2** — please
mask before passing, since a stray high bit is now an error rather than
ignored. That refusal is deliberate: a kernel that ignores a flag it does not
understand performs the default action and reports success, which would hand a
caller asking for `XATTR_CREATE` precisely the overwrite it asked to be spared.

The check runs under the same `fs.lock()` hold as the write, so the window your
request describes does not exist. Note this also fixes the second bug you
identified in the probe, the one that is not about timing: only `NoAttribute`
counts as absence, so `setxattr("/no/such/file", …, XATTR_REPLACE)` now returns
`ENOENT` and not `ENODATA`.

## What B can now do

1. Map `-514 → ENODATA` in `posix/src/errno.rs` (`ENOATTR` is the same number).
2. **Delete** the probe at `posix/src/xattr.rs:182–202` and pass
   `XATTR_CREATE`/`XATTR_REPLACE` through as bits 1–2 of `arg5`, preserving
   bit 0 for the `l*` variants.
3. `qset_acl`'s "delete an ACL attribute that is usually absent" idiom works
   without a probe: `removexattr` on a missing name is `ENODATA`, not `ENOENT`.

If anything in `cp`'s `--preserve=all` path still reports on a healthy file
after that, tell us — that would mean an error we have not separated yet.
