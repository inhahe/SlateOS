## 737. Which extended attributes are "permissions" is decided in the program, not read from `/etc/xattr.conf` — a missing config file must not silently change what a copy carries

**Date:** 2026-08-31
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** A file can carry extended attributes — small named blobs hung off
the file, like `user.comment` — and a couple of those names are not data *about*
the file but the file's own access list (an ACL: "and also let Bob read this").
`cp` has to sort them, because `--preserve=xattr` is supposed to carry the first
kind and `--preserve=mode` the second. On Linux the sorting is done by reading
`/etc/xattr.conf`, a file shipped by the distribution. This OS has no such file
and will not grow one for this: the two ACL names are written into the program.
The cost is that a site cannot add its own rules. The benefit is that the
sorting cannot go missing, which on Linux it demonstrably can — delete
`/etc/xattr.conf` there and `cp --preserve=xattr` quietly starts copying ACLs
that the user did not ask to copy.

### Where the split comes from

GNU does not have a list of permission-class names either. It has two callers of
libattr's `attr_copy_file`, which takes a *filter* callback, and libattr's
default filter (`attr_copy_action`) is a parser for `/etc/xattr.conf`:

| Caller | Filter | What it copies |
|---|---|---|
| gnulib `qcopy_acl` (from `--preserve=mode`) | `is_attr_permissions` | only names the config marks `permissions` |
| coreutils `copy_attr` (from `--preserve=xattr`) | `check_selinux_attr`, which defers to the config | everything the config does *not* mark `permissions`, minus SELinux's own |

So the classification is one table, consulted from two directions, and it lives
in a text file that neither program ships. The file distributions do ship marks
these as `permissions`:

```
system.posix_acl_access    permissions
system.posix_acl_default   permissions
system.nfs4_acl            permissions
system.nfs4acl             permissions
trusted.SGI_ACL_FILE       permissions
trusted.SGI_ACL_DEFAULT    permissions
```

### What the source says happens when the file is absent

libattr's fallback is not "assume the usual names". `attr_parse_attr_conf`
(`attr_copy_action.c`) opens `/etc/xattr.conf` and, on `ENOENT`, `return 0` —
success, with an empty rule list. `attr_copy_action` then finds no matching
pattern and returns `0`, which is neither `ATTR_ACTION_SKIP` (1) nor
`ATTR_ACTION_PERMISSIONS` (2). Every name gets the same non-answer, and the two
filters read that non-answer in opposite directions:

| Filter | Definition | Verdict with no config |
|---|---|---|
| `is_attr_permissions` (`qcopy-acl.c:36`) | `attr_copy_action (name, ctx) == ATTR_ACTION_PERMISSIONS` | false for **every** name |
| `check_selinux_attr` (`copy.c:751`) | not `security.selinux` **and** `attr_copy_check_permissions`, which is `attr_copy_action (name, ctx) == 0` | true for **every** other name |

So on a Linux box with no `/etc/xattr.conf`:

- `cp --preserve=xattr a b` copies `system.posix_acl_access` along with
  everything else, giving `b` an access list the user never asked to carry —
  `--preserve=xattr` is documented as precisely *not* being `--preserve=mode`.
- `cp -p a b` copies no ACL at all, because the permissions copy's filter now
  selects nothing: the ACL is silently dropped by the option whose whole job
  was to carry it.

The two failures point opposite ways from the same missing file, which is the
part that makes it a bad place for the rule to live: nothing about either
outcome announces that a config file was missing, and the two are not even
consistent enough to produce one recognisable symptom.

### The decision

`fsattr::Xattrs` is a two-valued enum — `Ordinary` and `Permissions` — and
`PERMISSION_XATTRS` is a two-element array of the names this kernel actually
stores. `Xattrs::carries` is the whole filter. There is no file to read, no
parse to fail, and no state in which the two halves overlap or leave a name to
both.

The array is two names and not the config file's six. The NFSv4 and SGI entries
name ACL flavours nothing in this tree implements; no code here can produce
those attributes, so listing them would be transcribing spellings that could
only ever be wrong in a way nobody would notice.

### Alternatives considered

- **Ship an `/etc/xattr.conf` and parse it, as Linux does.** The faithful
  choice, and the one to revisit if this OS ever grows a second ACL flavour or a
  reason for a site to exclude a name from copies. Rejected now because it buys
  configurability nobody has asked for at the price of a failure mode that is
  silent in both directions, and because the file would have to be shipped,
  found, versioned and documented — four moving parts to express six lines that
  no two distributions disagree about.
- **Parse it if present, fall back to the built-in list if not.** Strictly
  better than pure parsing and still worse than this: it keeps the parse and the
  divergence, and it means the answer to "what does `--preserve=xattr` carry?"
  depends on the machine. A user comparing two machines would be debugging a
  config file's absence, which is the exact thing the built-in list makes
  impossible.
- **No split at all — `--preserve=xattr` carries every attribute including
  ACLs.** What Linux does when the file is missing, and it makes
  `--preserve=xattr` quietly imply a slice of `--preserve=mode`. It also makes
  the eventual `--preserve=mode` ACL support unimplementable without taking the
  names back out again.

### What this does not decide

`--preserve=mode` does not yet carry the permission class; only `Ordinary` has a
caller. The `Permissions` half exists, is tested, and is waiting on an
attribute-*removal* primitive, because gnulib's `qset_acl` narrows a destination
by deleting its ACL and not merely by overwriting it. See the comment on
`narrow_before_chown` in `cp.rs`, which records the finding so that whoever
picks it up does not re-derive it wrongly.
