## 660. An xattr name is bytes, and "no such attribute" is not "no such file"

**Date:** 2026-08-31 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** A file can carry little labels alongside its contents — an
"extended attribute", used for things like access-control lists or the tags
our own `fs::tags` stores. Two things about them were wrong. First, the kernel
insisted the *name* of such a label be valid text, even though the disk stores
it as raw bytes and a disk written by Linux may well hold a name that is not;
worse, the check ran inside the loop that reads *every* label on a file, so one
odd name made the whole file's labels unreadable. Second, asking for a label
that isn't there gave the same answer as asking about a file that doesn't
exist, so a program could not tell "this file has no ACL" (ordinary, act
quietly) from "this file is gone" (report it). Both are now fixed: names are
bytes end to end, and a missing label answers `NoAttribute` (`ENODATA`) while a
missing file still answers `NotFound` (`ENOENT`).

Filed by lane B, whose `getfattr`/`setfattr` work needed both.

### The name is bytes, for the same reason a path is

`design.txt` already settles the general question: paths allow every byte
except `/` and NUL, and the OS-boundary rule in `CLAUDE.md` §7 says never to
force UTF-8 on data crossing that boundary. An xattr name is the same kind of
object — a NUL-terminated byte string the kernel stores and returns but does
not interpret. The namespace prefix (`user.`, `trusted.`, `security.`,
`system.`) is ASCII and is compared byte-wise, which needs no text type.

What made this more than a purity argument is **where** the rejection sat:

```rust
// kernel/src/fs/ext4/driver.rs:3508, before
let name = core::str::from_utf8(name_bytes).map_err(|_| KernelError::IoError)?;
```

That `?` is inside the loop over every attribute on the inode, so a single
non-UTF-8 name did not merely make *that* attribute unreachable — it returned
`EIO` for `read_all_xattrs`, and therefore failed `get_xattr`, `set_xattr`,
`remove_xattr` and `list_xattrs` for the whole file, including for attributes
whose own names were entirely ordinary. The inline-xattr parser had the same
defect in a quieter form: it `break`s out of the loop instead, silently
truncating the list at the first odd name, so the caller was told the file has
fewer attributes than it has and could not tell that anything was hidden.

The alternative — keep `&str` and reject odd names at the syscall boundary —
was rejected because it does not describe a filesystem we can actually mount.
We can be handed an ext4 volume written by Linux; refusing to read it correctly
is our bug, not the volume's.

### `NoAttribute` is a separate variant, not a reuse of `NotFound`

Linux spends two errno values here — `ENODATA` for the attribute, `ENOENT` for
the path — and it does so because programs branch on the difference. With one
code for both, a caller must either complain about a healthy file or stay
silent about a vanished one; it cannot do both correctly, because it cannot
tell them apart.

Our own tree already had a victim. `kernel/src/fs/tags.rs`'s `read_tags` had:

```rust
Err(KernelError::NotFound) => Ok(Vec::new()),
```

meaning a mistyped path reported "this file has no tags" rather than "there is
no such file" — `list` printed nothing and `has` answered a confident `false`.
That arm is now `Err(KernelError::NoAttribute)`, and a genuine `NotFound`
propagates. The same correction applies to `write_tags`'s "already gone" arm.

`NoAttribute` is `-514`, mapped to `ENODATA` in `syscall/linux.rs`. Note that
Linux makes `ENOATTR` an alias of `ENODATA` — the two spellings are one number,
so there is no second variant coming.

**Where the boundary sits:** the attribute lookup only happens *after* the
inode has been resolved, so the filesystem always knows which of the two it
hit. `NotFound` continues to mean the path did not resolve; `NoAttribute` means
it resolved and carried no such name.

### What this cost elsewhere

`read_user_cstring` was the only thing forcing UTF-8 at the syscall boundary,
and all three of its callers were xattr keys, so it became `read_user_cbytes`
and the `String::from_utf8` went with it. No ABI changed: `sys_fs_list_xattrs`
already packed NUL-terminated bytes into the user buffer.

`kshell`'s `xattr list` now writes keys with `shell_write_bytes` rather than
formatting them as text — deliberately not `from_utf8_lossy`, which maps every
undecodable byte to the same U+FFFD, so two distinct attributes would list
under one displayed name and neither could be fetched by what was printed.
That is the same reasoning already recorded for `diff` and `column`.

### Testing

The `fs::handle` no-follow xattr self-test asserted only that reading the
target's attribute returned *some* error, which is precisely the assertion that
cannot see this bug. It now requires `NoAttribute` for an existing file with no
such attribute, and `NotFound` for a path that does not resolve — the two calls
that were indistinguishable before.
