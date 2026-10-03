## 659. A hard link's source is gated for *read* and its destination for *write* — and until now neither was gated at all

**Decided by:** Claude (autonomous)
**Lane:** A
**Date:** 2026-08-31
**Area:** `kernel/src/fs/vfs.rs` — `link_inner`, `link_at_pinned`

**In short:** A "hard link" is a second name for a file that already exists —
both names are equally real, and deleting one leaves the file reachable through
the other. This system has a rule list saying which files a program is allowed
to touch (ACLs and capability tags, checked by one function,
`check_path_access`). Hard links were skipping that list entirely: a restricted
program could give a file it was forbidden to open a *second* name inside a
folder it was allowed to open, and then read it there. That is now checked. The
choice being recorded is which permission each of the two names needs — the new
name needs permission to **write**, the original needs only permission to
**read**.

### How it was missed

`Vfs::link` and `Vfs::link_no_follow` are thin wrappers; all the work is in a
private `link_inner`. `scripts/check-vfs-permission-gate.py` flags a function
that resolves a path without calling `check_path_access`, and it looks at
`pub fn`s on `Vfs` — so the wrappers (which resolve nothing themselves) passed,
and the private back-end that resolves both paths was never examined. Every
other mutation in the file was gated correctly; `rename_inner`, the closest
analogue, takes `Write` on both of its paths.

It surfaced only because the gate *did* flag the new `link_at_pinned` (§658),
which is a `pub fn` and therefore in scope. The pinned route was written to
match the path route deliberately, on the reasoning that two routes enforcing
different policies is worse than both enforcing one permissive policy — so the
gate caught the old hole by way of the new code copying it. That is the gate
working as intended, one call site later than would have been ideal.

### The decision: `Read` on the source, `Write` on the destination

| Path | Gate | Why |
|---|---|---|
| destination (the new name) | `PathAccess::Write` | a directory entry is being created there |
| source (the existing file) | `PathAccess::Read` | after the link, the caller reaches the object under a name they chose |

**Why not `Write` on the source, matching `rename`?** Because a rename *removes*
the source name and a link does not — it only raises the link count. Requiring
`Write` would forbid hard-linking a file you are allowed to read but not
modify, and that is the principal legitimate use of hard links: content-
addressed stores (this tree has one in `pkg/`), `cp -l`, and dedup-based
backup all link read-only sources. A gate that breaks the main honest use is
not a safer gate, it is one that gets turned off.

**Why gate the source at all, when POSIX `link(2)` needs no permission on it?**
Because this is not the POSIX DAC check — it is the sandbox's path policy, and
the two ask different questions. POSIX asks "may this uid do this operation";
the path policy asks "is this program allowed near this file at all". A
sandbox that denies `/etc/shadow` is defeated by linking it somewhere the
sandbox permits, since the object afterwards carries the *new* name's policy.
Read is the weakest gate that closes that, which is why it is the one chosen.

Both checks run against the **resolved** paths, after `follow` has been applied,
so a symlink cannot make the gate judge one object while the link is made to
another.

### What could break

Any caller that hard-links a source outside its own path policy now gets
`PermissionDenied` where it previously succeeded. That is the point, but it is
a behaviour change on a shipped path (`SYS_FS_LINK`), not only on the new
pinned one. In practice nothing in-tree is affected: `check_path_access`
returns `Ok` immediately when no ACL and no file tag exist anywhere, which is
the state of every boot today, and kernel tasks bypass it before any check
runs.

### Test

`acl_gate_self_test` gained a step asserting the **read/write split** that the
choice depends on: the same third-party uid that step 4 shows may *read* the
file is refused `Write` on it. Under a `Write`-based source gate that user
could not hard-link a file they are plainly allowed to read; under the chosen
`Read`-based gate they can. The pair is the decision, so both halves are
asserted rather than only the deny.

The call sites themselves are covered by `check-vfs-permission-gate.py`, which
now reports zero findings — a self-test could not cover them, because the
kernel's self-tests run as a kernel task and `check_path_access` bypasses those
before any check runs.
