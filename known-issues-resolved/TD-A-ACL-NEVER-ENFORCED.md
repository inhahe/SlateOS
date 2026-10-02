## TD-A-ACL-NEVER-ENFORCED — POSIX ACLs are stored, listed and formatted, but no VFS operation ever consults them (lane A, 2026-08-23)

**Status: RESOLVED 2026-08-23** in `f838b82cb` (the gate),
`f69f16638` (the checker that keeps it complete) and `befd76f9e` (making
the gate's fast path affordable). Discovered while converting
`kernel/src/fs/acl.rs` to byte-clean paths (design-decisions.md §261).
The original report is kept below, followed by what was actually built.

`fs::acl::check_access` — the function that implements the whole POSIX
1003.1e evaluation algorithm, all five steps of it — **has no production
callers.** The evidence is a one-liner:

```bash
grep -rn "check_access" kernel/src --include=*.rs | grep -v "fs/acl.rs"
```

Every hit is a different, unrelated `check_access`: `cap/file_tags.rs`,
`cap/groups.rs`, `fs/appsandbox.rs`. Nothing in `fs/vfs.rs` — or anywhere
else — calls `fs::acl::check_access`. The only callers of the *rest* of
the module are `kshell`'s `getfacl`/`setfacl` commands and `procfs`'s
statistics line.

**What this means in practice.** `setfacl` appears to work: it validates
the ACL, stores it, and `getfacl` reads it back verbatim. The statistics in
procfs will report `files_with_acls: N`. But the ACL governs nothing — an
open/read/write/unlink goes through the VFS's traditional owner/group/other
check and never learns the ACL exists. A user who denies a colleague access
to a file with `setfacl -m u:1001:--- /path` is told the operation
succeeded and is given no indication that user 1001 can still read the
file. **This is a security feature that reports success while doing
nothing**, which is worse than not having the feature: the absence of a
feature is visible, a silently-inert one is not.

Note also that `check_access` **fails open** by design — `None => return
Ok(())`, deferring to traditional permissions — which is the right
behavior for a hook that runs on every path, but it means that wiring it in
incorrectly (e.g. passing an unnormalized or relative path, so the lookup
misses) degrades silently to "no ACLs at all" rather than to a visible
failure. Whoever does the wiring must test the *deny* direction, not just
that nothing broke.

**The proper fix** is to call `fs::acl::check_access` from the VFS
permission check, on the same resolved absolute path the rest of the
operation uses:

1. Find the single point in `fs/vfs.rs` where traditional
   owner/group/other permission is evaluated for a path operation. If there
   is no single point, make one first — the ACL hook must not be sprinkled
   across every entry point, or the next entry point added will forget it.
2. Call `acl::check_access(path, uid, gid, file_uid, file_gid, request)`
   *after* the traditional check grants access, never instead of it: POSIX
   ACLs can only be evaluated once the owner/group of the file is known,
   and an ACL that grants must not override a mount flag (`ro`, `noexec`)
   or an immutable/append-only bit that already refused.
3. The path passed must be the normalized absolute path, since that is what
   `set_acl` keys on. `fs/vfs.rs`'s `normalize_mount_path` is the model.
4. Test the deny direction end-to-end from `kshell`: `setfacl` a deny for a
   non-owner uid, then confirm the read actually fails. `acl.rs`'s own
   self-test (11 tests) covers the algorithm but cannot cover the wiring.

**Trigger to promote this to active work:** any task that touches VFS
permission checking, or any task that claims POSIX ACL support is done.
Until then, `getfacl`/`setfacl` should be understood as a database editor
for a database nothing reads.

### Resolution (2026-08-23)

All four numbered steps above were followed, and step 1's parenthetical
turned out to be the larger half of the work.

**There was no single point, and the reason there wasn't is instructive.**
`cap::file_tags::check_access` — the *other* path check, mandatory access
control by capability tag — was called from sixteen hand-written sites in
`fs/vfs.rs` plus a seventeenth hand-copied into `fs/handle.rs`. It was also
*missing* from roughly twenty more entry points: every xattr getter and
setter, `chmod`/`chown`/`utimes` and their `no_follow` twins, `symlink`,
`readlink`, `lstat`, `lmetadata`, `truncate_resolved`, `fallocate`,
`file_identity_resolved` and `readdir_at_resolved`. So the sprinkled hook
had already decayed exactly the way step 1 predicted a new one would, and
adding an eighteenth sprinkle would have decayed the same way. Both checks
now live in one function:

```rust
check_path_access(path, PathAccess::{Read, Write, Execute, Metadata})
```

**`PathAccess::Metadata` requires no ACL permission in either direction.**
That is a deliberate deviation from "gate everything", and both halves of
it match POSIX: `stat` depends on search permission along the path rather
than read permission of the target, so requiring `r` would make a file an
ACL had deliberately left visible-but-unreadable also un-`stat`-able; and
POSIX ACLs govern a file's *data*, not its inode attributes, so requiring
`w` would make a mode-`444` file un-`chmod`-able by its own owner.
Capability tags still apply to metadata operations, since those are
mandatory access control and deny reaching the object at all.

**The recursion hazard step 2 implies.** The ACL check needs the file's
owning uid/gid, which means the gate reads metadata — so it must use
`Vfs::metadata_resolved` (ungated) and never `Vfs::metadata` (gated). The
checker asserts this as a named rule rather than leaving it to memory.

**Step 4 could not be done as written, and what replaced it is stronger.**
An end-to-end `kshell` test is impossible from inside the kernel: self-tests
run as a kernel task with no owning process, so `check_path_access` bypasses
before any check runs, and a test driven through `Vfs::read_file` could only
ever observe "allowed" — it would keep passing if the ACL half were deleted
again. The decision logic was therefore split into a pure
`path_access_verdict(path, uid, gid, supplementary_gids, want)` that takes
credentials explicitly, and `vfs::acl_gate_self_test` asserts the **deny**
direction against it first, exactly as the report demanded. The *call sites*
— which a unit test cannot cover — are held by
`scripts/check-vfs-permission-gate.py`, run as a pre-build gate in
`boot-test.sh`. It is the right tool for this specific failure precisely
because both checks fail open: a forgotten gate produces no symptom at all
at runtime, so only a source-level invariant can see it.

**Fast path.** The gate asks `acl::count()` and `file_tags::count()` before
doing anything, and on a normal system both are zero. `acl.rs` gained an
atomic count for this; `file_tags::count()` was a mutex plus an O(n) scan of
the whole table, which was affordable at seventeen call sites and is not at
thirty-five on a lookup path budgeted at 200–500 ns per component, so it
gained the same treatment (`befd76f9e`). See design-decisions.md §290.
