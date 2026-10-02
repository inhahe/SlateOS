## 738. `--preserve=mode` replaces the destination's access-control list rather than merging the source's into it, which is GNU's careful branch and not its fast one

**Date:** 2026-08-31
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** An access-control list, or ACL, is a per-file list of extra
permission grants — "user carol may also write this file" — that sits beside
the ordinary owner/group/other bits and can grant access those bits do not
show. When you run `cp --preserve=mode a b` and `b` already exists, `b` may be
carrying such a grant. GNU coreutils answers two different ways depending on
which optional library it was compiled against: one build leaves carol's grant
on `b`, the other takes it off. This `cp` takes it off. The reason is that
`--preserve=mode` means "give `b` the permissions `a` has", and leaving a grant
`a` does not have makes that sentence false in the direction that matters — `b`
ends up readable or writable by somebody the user believes they have just
locked out.

### Where the two answers come from

Both live in gnulib's `qcopy_acl` (`lib/qcopy-acl.c`), one in each half of a
`#ifdef USE_XATTR`. `USE_XATTR` is on when the build found **libattr**, the
library that copies extended attributes wholesale.

| Build | What `--preserve=mode` does to the destination |
|---|---|
| **libattr present** | `chmod` to the source's mode, then `attr_copy_file(…, is_attr_permissions, …)` — copy the ACL attributes the *source* has. A name the source lacks is not mentioned, so a grant on the destination stays. |
| **libattr absent** | `get_permissions` from the source, `set_permissions` onto the destination. That path *writes* the destination's access ACL — from the source's, or, when the source has none, from the mode — and so replaces whatever was there. |

The comment above the first is candid about being a shortcut: *"Rather than
fiddling with acls one by one, we just copy the whole ACL xattrs."* It is
faster, and on the case it was written for — a destination this run just
created, which has no ACL of its own — the two agree exactly. They part company
only when the destination pre-exists and carries a grant the source does not.

That the second branch is the intended semantics is not inference. gnulib's
`qset_acl`, which the same file's sibling calls, documents itself as *"Set the
access control lists of a file to match **exactly** MODE (this might remove
inherited ACLs). Note chmod() tends to honor inherited/default ACLs."* Removal
is the stated contract; the libattr branch simply does not implement it.

### What the divergence costs

A chmod does not take an ACL off. The kernel rewrites the list's owner, mask
and other entries to match the new bits and **keeps every named-user and
named-group entry** — that is what `qset_acl`'s "chmod() tends to honor
inherited ACLs" is warning about. So under the libattr branch:

```
setfacl -m u:carol:rw b      # carol may write b
chmod 600 a                  # a is the owner's alone, and has no ACL
cp --preserve=mode a b       # "give b the permissions a has"
```

leaves `b` at mode 0600 *and* still writable by carol. Nothing printed, nothing
in `ls -l` beyond a `+` most people do not read, and the `cp` reported success.
Under the other branch carol's entry is gone.

This is not a hypothetical shape. The same replacement is what makes the
narrowing step in `set_owner` safe — `cp -p` onto an existing file drops the
mode to `old & new & 0700` before handing the file to a new owner, precisely so
the old permissions cannot outlive the old owner, and an ACL entry that survived
that chmod would defeat the whole step. GNU narrows with `qset_acl` for exactly
that reason, and its condition is `USE_ACL || …`: with ACLs compiled in it
narrows *unconditionally*, because the mode-bit test it would otherwise use
cannot see an ACL. This tree therefore already depends on removal being real in
one place; having `--preserve=mode` not do it in another would be incoherent.

### The decision

`fsattr::copy_permissions` — `cp`'s `--preserve=mode` call — is chmod, then
delete both `system.posix_acl_access` and `system.posix_acl_default` from the
destination, then copy whichever of those two the source has. The end state is
"the destination's ACLs are the source's ACLs", identical to the non-libattr
branch and to the plain reading of the option.

`fsattr::set_mode_exactly` — the narrowing, and `--no-preserve=mode` — is chmod
plus deletion of the **access** list only. The default list, which decides what
a directory hands to files created inside it later rather than who may use the
directory now, is left alone. That is GNU's behaviour too, though by accident of
a sort worth recording: its `set_acls` deletes the default list under
`S_ISDIR (ctx->mode)`, and every one of `cp`'s calls passes permission bits with
no file-type bits in them, so the test never fires.

Deleting the attribute is how a minimal ACL is written here. gnulib builds a
three-entry list from the mode and writes it; Linux and this kernel both store
such a list as plain mode bits and drop the attribute, so the two are the same
operation, and deletion is the one expressible without an ACL encoder this tree
does not need for anything else.

### Alternatives considered

**Match the libattr branch exactly, since it is what a modern Linux `cp` does.**
The argument for it is real: bug-compatibility is a feature in a compatibility
utility, and a script that has learned GNU's behaviour is a script this would
break. Rejected because the behaviours being matched *disagree with each other*
— there is no single "what GNU does" here, so compatibility cannot be the
deciding argument, and between two GNU answers the one to take is the one whose
documentation says it is the intent. The cases where they differ are also the
cases where the difference is a permission left standing, which is the failure
direction that costs something.

**Delete both names in `set_mode_exactly` too, so there is one operation
instead of two.** Simpler, and on a regular file the extra deletion is a
harmless `ENODATA`. Rejected because it is wrong on the one case it reaches: a
directory created by `cp -r --no-preserve=mode` inherits a default ACL from its
parent, that inheritance is the parent's deliberate policy about what goes
inside it, and `--no-preserve=mode` says nothing about it. GNU keeps it, and so
does this.

**Leave `--preserve=mode` as a chmod and treat ACLs as out of scope.** This is
what shipped until now, and the comment on `narrow_before_chown` recorded it as
a known gap rather than a decision. Rejected because this kernel enforces POSIX
ACLs (`kernel/src/fs/acl.rs`, checked from `vfs.rs`'s permission path), so an
unrestored grant is a real grant here and not a portability abstraction.

### What this does not decide

Nothing reads or writes an ACL's *contents* — the two attributes are moved and
deleted as opaque bytes. A future `setfacl`/`getfacl`, or a `cp` that had to
translate between ACL flavours, would need an encoder; gnulib needs one only
because it must produce a minimal list where this tree can delete instead.

The differential harness cannot exercise any of this: the reference build has
`USE_ACL 0` as well as no `USE_XATTR`, so it has no ACL behaviour to compare
against, and the development host ships no `setfacl` to build a fixture with.
The coverage is in `fsattr`'s tests, which synthesise the on-disk ACL by hand
and skip themselves if the host filesystem refuses it.
