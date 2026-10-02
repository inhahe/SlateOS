## TD-B-MKTEMPS-ACCOUNT-LOOKUPS-CANNOT-TELL-ABSENT-FROM-UNREADABLE (lane B, 2026-09-11) — **CLOSED** 2026-10-02

**Status:** CLOSED 2026-10-02 — the crate is gone. `userspace/mktemp` was
deleted when `mktemp` became coreutils 9.4's port, and `id`, `whoami` and
`groups` had already been coreutils' (`userspace/coreutils/src/bin/id.rs` and
its neighbours) since §1005. Those ports also read an unreadable database as an
empty one, and that is upstream's behaviour rather than this defect again:
glibc's `getpwnam` answers NULL when `/etc/passwd` cannot be read, and `id.c`
turns any NULL into `'alice': no such user` without consulting `errno`. The
four `mktemp:` lines left `scripts/read-defaults-baseline.txt` with the crate.

**In short:** `userspace/mktemp` -- which is also `id`, `whoami` and `groups` --
reads `/etc/passwd` and `/etc/group` with `Err(_) => return Vec::new()`. An
unreadable database is therefore an empty one, so `id alice` answers "no such
user" with confidence when the real problem is that it could not look.

**Where.** `read_passwd` and `read_group` in
`userspace/mktemp/src/main.rs`. The four `mktemp:` entries in
`scripts/read-defaults-baseline.txt` sit on top of these and are NOT the defect:
`uid_to_name(uid).unwrap_or_default()` feeds an `is_empty()` check that prints
`uid=1000` with no name, which is exactly what real `id` does for an account
with no record. They stay pinned.

**Why it is an entry and not a same-day fix.** The visible cost is
degradation, not a wrong decision: `id` and `groups` print numbers instead of
names. The one confidently-wrong answer is `id <name>` reporting no such user.
Nothing here grants or refuses anything.

The same line in `userspace/doas` DID guard a privileged action -- an
unreadable `/etc/group` silently deleted every `deny :group` rule -- and was
fixed on 2026-09-11. That is the difference worth keeping: the identical
`Err(_) => Vec::new()` is a cosmetic defect in one crate and a policy
inversion in the other, and only reading what sits above it tells you which.

**The proper fix** is `Option<Vec<..>>` from both readers, as `doas` now has,
with `id`'s output distinguishing "this uid has no account" from "the account
database could not be read" -- the second deserves a diagnostic on stderr, not
a silently numeric line.

**The rest of the tree was swept for this shape and is clean.** Ten sites
collapse an unreadable account or policy file into an empty collection:

| Crate | Verdict |
|---|---|
| `doas` | **real defect** -- deleted every `deny :group`. Fixed 2026-09-11. |
| `mktemp` | this entry: display only, `id`/`whoami`/`groups` print numbers |
| `getent`, `loginctl`, `fuser` | display, or fail closed -- `loginctl`'s `require_user` exits when the lookup finds nobody |
| `newgrp` | **already correct, deliberately** |

`newgrp` is worth reading rather than re-deriving. Its `read_gshadow_db`
carries the reasoning in a doc comment -- "An unreadable file is an empty list,
and an empty list refuses everyone ... `newgrp` runs setuid root precisely so
that it *can* read this file; a caller that cannot is not a reason to admit
anyone" -- and `password_opens` returns `false` both for a group with no entry
and for one whose password field is empty. Its `/etc/group` read fails the same
way: nobody is a member, so the password is demanded rather than skipped. The
rule is extracted from the file read specifically so every branch is reachable
from a test, with a note that a stub accepting any password once survived there.

So the shape is not on its own a defect. What decides is whether a DECISION or
a DISPLAY sits above it, and in nine of ten cases here the answer was benign.
