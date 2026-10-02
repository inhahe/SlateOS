## §290 — Access control lives in one gate that every path operation must pass, and a build-time checker holds the door rather than a code review
**Date:** 2026-08-23
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel had two separate permission checks on files. One of
them — POSIX ACLs, the thing `setfacl` edits — was never actually consulted by
anything, so telling the system "user 1001 may not read this file" appeared to
work and did nothing. The other — capability tags — was consulted, but only in
the ~17 places somebody had remembered to write it, out of ~37 places that need
it. Both are now decided in a single function that every file operation calls,
and a script run before each build fails the build if a new file operation is
added without it. The decision recorded here is the *single gate plus checker*
shape, and the choice to exempt metadata operations from ACLs.

### What was there

`fs::acl::check_access` implements the full POSIX 1003.1e algorithm — owner,
named user, owning group, named group, mask, other, with mask intersection at
each named step. It is 11-self-test correct and had **zero production callers**.
`cap::file_tags::check_access` (mandatory access control by capability group)
had seventeen, hand-written one at a time at the top of seventeen `Vfs` methods,
with a copy in `fs/handle.rs`.

The two failures look opposite but are the same failure. A check that must be
re-typed at each entry point will be present at the entry points that existed
when someone last swept, and absent from every one added since; run that process
long enough with no forcing function and you get both seventeen copies *and*
twenty omissions *and* one check that nobody ever started.

### Why nothing noticed

Both checks **fail open**: `acl::check_access` returns `Ok(())` when the path
has no ACL, `file_tags::check_access` returns `Ok(())` when it has no tags. That
is correct — they are hooks on every path and must defer to traditional
permissions — but it means a missing call site has *no runtime symptom*. Files
open, read and write exactly as they did. Any test that exercises permitted
access passes identically whether the gate is there or not.

This is the property that decides the design. A defect with no symptom cannot be
caught by testing behaviour; it can only be caught by asserting something about
the *source*. Hence:

### The decision

1. **One gate.** `vfs::check_path_access(path, PathAccess)` runs both checks.
   Nothing else in the kernel may call either underlying `check_access`.
2. **The intent is passed in, not inferred.** `PathAccess::{Read, Write,
   Execute, Metadata}` — because the two checks disagree about granularity
   (a tag is all-or-nothing on a path; an ACL grants r, w and x independently),
   and the call site is the only place that knows which it means.
3. **The decision half is separated from the credential half.**
   `path_access_verdict(path, uid, gid, supplementary_gids, want)` takes
   credentials explicitly; `check_path_access` looks them up from the current
   process and calls it. This exists so the logic is testable at all — see below.
4. **A pre-build checker enforces 1 and the completeness of the call sites.**
   `scripts/check-vfs-permission-gate.py`, wired into `boot-test.sh`.

### The alternative, and why it lost

The obvious alternative was to put the ACL check inside the existing traditional
permission check and leave the sprinkled tag checks alone — smaller diff, no new
script, no new enum. It fails on the evidence already in the tree: the sprinkled
arrangement had *already* decayed to 17-of-37 with nobody noticing, so adding an
eighteenth sprinkle predicts its own future. The second alternative — one gate
but no checker — is better and still insufficient, because the property that
makes the bug invisible at runtime (failing open) is unaffected by how tidy the
code is today. Only something that reads the source each build keeps it tidy.

The cost is real and worth stating: the checker is a textual within-file
heuristic (like its siblings `check-vfs-under-lock.py` and
`check-recursive-locks.py`), it keys rule 3 off `resolve_mount` as the marker
for "reaches a real filesystem", and so it has false negatives by construction.
It is a ratchet, not a proof. A ratchet that catches the *next* forgotten entry
point is worth more than a proof nobody can run.

### The exemption that is not an oversight: `PathAccess::Metadata`

Metadata operations — `stat`, `readlink`, `lstat`, and also `chmod`, `chown`,
`utimes`, attribute flags — pass the tag check but require **no ACL permission
in either direction**. Both halves match POSIX and neither is obvious:

- *Reading:* POSIX makes `stat` depend on search permission along the path, not
  on read permission of the target. Requiring `r` would make a file that an ACL
  deliberately left visible-but-unreadable also un-`stat`-able, which no Unix
  does.
- *Writing:* POSIX ACLs govern a file's **data**; who may change the inode's
  mode and ownership is decided by ownership and privilege. Requiring `w` would
  make a mode-`444` file un-`chmod`-able by its own owner.

Capability tags still apply to metadata, because they are mandatory access
control: a tag denies reaching the object at all, however innocuous the intent.

The exemption list in the checker (`statvfs`, `trim`, `set_volume_label`, the
mount-table methods) is the same idea at the other end — those act on the
*filesystem*, with the path serving only to select which mount. Each entry
carries prose rather than a bare name specifically so the claim can be re-argued
if the method changes role, instead of being inherited silently.

### Why the self-test drives `path_access_verdict` and not a real VFS call

Kernel self-tests run as a kernel task with no owning process, and
`check_path_access` bypasses when it cannot find one — before any check runs. A
test driven through `Vfs::read_file` could therefore only ever observe
"allowed", and would go on passing if the ACL half were deleted again. That is
the same non-symptom as the original bug, reproduced inside the thing meant to
detect it.

So the test calls the pure verdict function with synthetic credentials, and
asserts the **deny** direction first. That ordering is not stylistic: because
`check_access` fails open, a test that only confirms ordinary access still works
passes just as well against a gate that was never wired in. The call sites,
which a unit test cannot reach, are the checker's job. The two halves are
complementary and neither alone would have caught the original state.

### A consequence worth recording: the fast path had to get cheaper

The gate's first act is `acl::count()` and `file_tags::count()`, to skip
everything when no ACLs or tags exist — the normal case. `file_tags::count()`
took a global mutex and scanned the whole table. At seventeen call sites that
was tolerable; at thirty-five, on a lookup path budgeted at 200–500 ns per
component, it would have made every unrelated file operation in the kernel
contend for one lock. Both counts are now relaxed atomic loads.

A relaxed load is sound because the count is only ever a *filter*, never the
answer: a stale non-zero costs one redundant scan under the lock, and a stale
zero can only be read by a thread whose operation was already racing the
`tag_path` that would have denied it — the same race the lock would have had.

The residual risk is that a future mutator forgets to refresh the cache, and a
stale zero there is not a slow answer but a wrong one: the gate reads it as "no
tags exist" and skips mandatory access control entirely. `file_tags` self-test 7
therefore asserts the cache against a fresh scan of the table after every kind
of mutation the module offers. The cache is recomputed absolutely rather than
incremented, because a missed decrement drifts silently while a recompute
cannot.
