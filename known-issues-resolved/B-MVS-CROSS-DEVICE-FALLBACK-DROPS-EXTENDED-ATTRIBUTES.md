## B-MVS-CROSS-DEVICE-FALLBACK-DROPS-EXTENDED-ATTRIBUTES — FIXED 2026-09-01

**In short:** a file moved across a filesystem boundary arrives without its
extended attributes. Its times, owner, mode and access-control lists are carried
now; everything else in the `user.*`, `trusted.*` and `security.*` namespaces is
not. On a machine that stores nothing there this is invisible; on one that does,
`mv` silently strips whatever was stored — a SELinux label, a `user.mime_type`, a
backup tool's bookkeeping — and there is no diagnostic, because nothing looked.

**Where.** `userspace/coreutils/src/bin/mv.rs`, `preserve_onto_file`. It runs
`fsattr::set_times`, `fsattr::take_ownership` and `fsattr::copy_permissions`, and
the last of those carries `Xattrs::Permissions` only — the ACL attributes, which
are part of the mode's meaning. There is no `Xattrs::All` step, and
`preserve_onto_link` has none either.

**What GNU does.** `cp_option_init` (`mv.c:129`) sets `preserve_xattr = true`
with `require_preserve_xattr = false`, so `copy_reg` calls `copy_attr` between
the `set_owner` and the `copy_acl` (`copy.c:1668`) and a failure is a diagnostic
rather than a failed move. There is a second, smaller thing tied to it:
`copy.c:1457` ORs `S_IWUSR` into the mode the destination is created with, when
copying attributes as a process that does not own the file, because the kernel's
`xattr_permission` wants write access to the inode. `create_destination`'s doc
says why that bit is deliberately not there yet.

**The proper fix.** A `fsattr::copy_xattrs(On::File(source), On::File(dest),
Xattrs::All)` between the ownership step and the permissions step, reporting each
`XattrError` the way `cp` does and failing at none of them, plus the `S_IWUSR`
widening in `create_destination`. `cp` already has all of the machinery —
`Xattrs`, `XattrStep` and `XattrError` exist in `fsattr` for it — so this is
wiring rather than new code. The ordering is not free: it must be after the
`chown`, because on Linux a `chown` can drop `security.capability`, and before
the mode, for the reason the whole tail is in that order.

**How it would be caught.** Not by `scripts/mv-diff.sh` as it stands: the harness
compares the tree, the bytes, the modes and the hard-link groups, and has no
notion of extended attributes at all. Catching it means teaching `snapshot` to
run `getfattr -d -m -` per file, which is worth doing in the same change and
would also cover `cp`'s xattr cases, currently unmeasured for the same reason.
Until then the unit tests in `mv.rs` are the place: they drive
`copy_across_devices` directly and can set an attribute on the source and assert
it on the destination, on a `/tmp` that supports one.

**Status:** FIXED 2026-09-01. `preserve_onto_file` gained a `preserve_xattrs`
call between the ownership step and the permissions step, `preserve_onto_link`
gained one after its `set_times`, and `create_destination` gained the `S_IWUSR`.
mv's own suite went 126 → 128 passed; `scripts/mv-diff.sh` is unchanged at 341
passed, 0 differed, 11 differ on purpose, which is what "the harness cannot see
this" above predicted and is therefore evidence rather than a shrug.

Three corrections to what this entry prescribed, all found by reading rather
than by assuming:

* **`Xattrs::All` does not exist**, and inventing it would have been wrong
  rather than merely unavailable. The right variant is `Xattrs::Ordinary` —
  everything *except* the permission class — because the permission class is
  `system.posix_acl_access` and its default counterpart, which the
  `copy_permissions` on the next line already carries. Copying them here as
  well would write the access list twice, and the second write would land
  *before* the mode that has to precede it.
* **The line numbers were a version adrift.** `cp_option_init` is `mv.c:119`,
  not `129`; the `preserve_xattr = true` inside it is `mv.c:145`; `copy_attr`
  is `copy.c:1662`, `copy_acl`'s arm is `copy.c:1672` rather than `1668`, and
  the `S_IWUSR` is `copy.c:1450` rather than `1457`.
  The claim they support is right; the citations were not, and a citation that
  does not resolve is worse than none.
  (Corrected again 2026-09-01: this bullet itself gave `cp_option_init` as
  `mv.c:145` — the same line as the `preserve_xattr = true` *inside* it, which
  is the tell that it was copied from the line below rather than read. Both are
  now verified against `coreutils-9.4/src/mv.c`, where `cp_option_init` opens at
  119 and its `preserve_xattr = true` is at 145. A correction is not exempt from
  the rule it is enforcing.)
* **`preserve_onto_link` needed one too, which reads like a contradiction of
  mv.rs's own doc and is not.** That doc explains at length that a symlink
  returns early at `copy.c:3285` and so never reaches the mode block. But
  `copy_attr` is at 3280 — *before* that return, at 3285–3286 — so a link passes
  through it on the way out. Whether anything is then carried is the
  filesystem's business (Linux permits only `trusted.` and `security.` on a
  link), so in the ordinary case the call copies nothing and costs nothing;
  what it buys is that the one namespace that *can* be set on a link is not
  silently dropped.

**The `S_IWUSR` is not decoration, and the test proves it.** With the widening
removed, `a_read_only_source_still_gets_its_attributes` fails with
`mv: setting attribute 'user.tag' for '…/b': Permission denied` — a `0444`
destination that Linux's `xattr_permission` will not let its own creator write
an attribute to. The same test asserts the bit does not *survive*: the mode at
the end is `0444` again, because a move takes `copy.c:1672`'s
`if (x->preserve_mode || x->move_mode)` arm and `copy_acl` writes `src_mode`
absolutely — so GNU's `extra_permissions` cleanup branch is unreachable for
`mv`, and ours needs no bookkeeping either, `copy_permissions` beginning with
the same absolute `set_mode`.

Both new tests were checked for discrimination rather than assumed to
discriminate: with the `preserve_xattrs` call removed both fail, and with only
the widening removed exactly the read-only one does.

**Still open, and deliberately not done here:** teaching `snapshot` in the diff
harnesses to run `getfattr -d -m -`. The paragraph above is still the argument
for it, and it is still worth doing — it is what would let the *harness* catch a
regression here instead of the unit tests, and it would cover `cp`'s xattr cases
too. Left out because it changes the harness for both utilities and belongs in
its own change; tracked as
`B-DIFF-HARNESSES-CANNOT-SEE-EXTENDED-ATTRIBUTES` below.
