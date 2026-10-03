## B-DIFF-HARNESSES-CANNOT-SEE-EXTENDED-ATTRIBUTES — FIXED 2026-09-01

**In short:** the two differential harnesses that compare our `cp` and `mv`
against the real GNU ones cannot see extended attributes — small named blobs a
filesystem stores alongside a file (a SELinux label, a `user.mime_type`, a
backup tool's bookkeeping). So a change that silently stopped carrying them
would show up as "0 differed". Both utilities *do* carry them, and both have
unit tests that say so; what is missing is the whole-program check.

**Where.** `scripts/cp-diff.sh` and `scripts/mv-diff.sh`, the `snapshot`
function in each. It records the tree, each file's bytes, its mode, and the
hard-link groups — everything an attribute is not.

**The proper fix.** Add `getfattr -d -m - --absolute-names` per file to
`snapshot`, with its output sorted and folded into the same text blob the rest
of the snapshot goes into, so a difference in attributes fails a case the same
way a difference in bytes does. Two things to get right:

* **`getfattr` may not be installed.** It is part of `attr`, which is not
  guaranteed. The harness should probe once and, if it is absent, say so in the
  summary line rather than silently comparing nothing — a harness that quietly
  stops checking something is worse than one that never checked it.
* **`security.selinux` will differ on a machine with SELinux enforcing**, for
  the same uninteresting reason the owner does. It belongs in the same category
  as the fields `snapshot` already elides.

Then add cases that actually set one: `setfattr -n user.tag -v v` in a `TREE`,
for both a plain file and a hard-linked pair, and — for `mv` — a `FAR` case, the
only shape where the fallback runs at all.

**How it would be caught.** It is the catcher; nothing catches it. The evidence
it is needed is that
`B-MVS-CROSS-DEVICE-FALLBACK-DROPS-EXTENDED-ATTRIBUTES` above was fixed with the
harness reporting an identical 341/0/11 before and after.

**Fixed 2026-09-01, in two halves, and the second half was the one that was not
foreseen above.**

The first half is the comparison: `diff-wsl.sh` section 8 adds `diff_xattrs_in`,
which both harnesses fold into `judge` beside the tree, the bytes and the
hard-link groups, and which reports as `xattr{…}`. It is **not** `getfattr`, as
this entry proposed. `getfattr` is part of `attr`, which is not installed on
this host and cannot be installed without a password the scripts do not have —
but the substitution is an improvement rather than a concession, for three
reasons set out at length in section 8: `os.listxattr(..., follow_symlinks=
False)` reads a symlink's own attributes without a flag that is easy to leave
off, the value stays bytes under one stated encoding rule instead of `-d`'s
per-value guess between text and base64, and no subprocess is needed per file.
`security.selinux` is elided, as suggested; the rest of `security.*` is not,
since `security.capability` is a real thing for these two programs to lose.

The second half is that **the reference could not carry an attribute either**,
which this entry did not anticipate and which would have made the first half
worse than useless: `m4/xattr.m4` had found no libattr on the build host, so the
reference `cp` and `mv` had `USE_XATTR` undefined and a `copy_attr` whose entire
body is `return true`. The first five cases written against it came back red
with *ours* carrying the attributes and GNU carrying none — a harness reporting
a difference in the subject that is entirely in the reference is worse than one
that reports nothing. `diff-wsl.sh` therefore builds libattr 2.5.2 from source
into the same cache coreutils is built in, and configures coreutils against it;
what was actually achieved is read back from the built tree's `config.h` rather
than assumed from the flags, because `gl_FUNC_XATTR` can decline and only warn.

Cases: mv-diff section 23, five of them, and cp-diff section 17, five more plus
the three `--preserve=xattr` cases that were `xfail_case`s naming this exact
shortfall and are now real. Measured rather than assumed, which is the test this
entry itself demands: with `preserve_xattrs` in `mv.rs` short-circuited to
`return`, mv-diff goes from 351/0 to 348/3 — before this work the same break
changed nothing at all.

Still not covered, for a reason outside the harness: an attribute on a directory
crossing a filesystem boundary, because a directory cannot cross one yet
(`B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED`). mv-diff section 23 names the
case to add when it can.

**Extended to access-control lists, 2026-09-01 (same day).** The half about the
reference turned out to have a second instance: without libacl, gnulib compiles
`copy_acl` down to a plain `chmod`, so the reference carries a file's mode bits
and silently drops its ACL entries. That failure is *quieter* than the libattr
one — a reference without libattr refuses `--preserve=xattr` outright, which is
loud, while this one copies happily and is merely wrong — so it is the shape
more likely to be mistaken for a bug in the subject. `diff-wsl.sh`'s libattr
block was generalised into a pair (`diff_dep_links`, `diff_dep_build`) that
builds any small autotools dependency into one shared prefix, and libacl 2.3.2
goes through it; `DIFF_ACL_REF` is read back from `config.h`'s `USE_ACL 1`, and
`DIFF_SETFACL` is the separate fact of having a tool that can *make* one.

No new comparison was needed: on Linux an ACL is stored as the extended
attribute `system.posix_acl_access`, so `diff_xattrs_in` already reads it byte
for byte. There is deliberately no `diff_getfacl` — a second, weaker view of a
thing already compared exactly is worse than one that cannot disagree with
itself.

Cases: cp-diff section 17, seven (four requiring the list to arrive, two
requiring it *not* to — `--preserve=xattr` alone and bare `cp` — and one with an
ACL and an ordinary attribute on the same file); mv-diff section 24, six.
Measured: short-circuiting the permission-attribute copy in
`fsattr::copy_permissions`, which is exactly a gnulib built without libacl,
takes cp-diff from 572/0 to 567/5 and mv-diff from 357/0 to 353/4, and in both
the cases that move are exactly the ones that should. The same directory gap
applies: mv's cross-device directory default-ACL case waits on the same issue.
