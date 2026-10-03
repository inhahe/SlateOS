## 957. "Metadata belongs to the file" is too coarse to key a table by; ask whether the data should survive the file being replaced

**Date:** 2026-09-21 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** A

**In short:** several kernel tables attach information to a file -- who may
read it, a comment, a tag, a hash of its contents. Each has to be stored under
some key, and the two candidates are the file's *name* and the file's *identity*
(its inode). I had been choosing with the rule "data about a file should follow
the file, so use the inode", and that rule is wrong often enough to be
dangerous: applied to integrity monitoring it would have silently destroyed the
thing being monitored. The better question is not what the data is *about* but
what should happen to it when the file at that name is **replaced**.

**The case that broke the old rule.** `fs::integrity` stores a content hash per
file and `verify_file` reports whether the file still matches. By the old rule
that is per-file data and should be inode-keyed. But the threat integrity
monitoring exists to catch is a file being swapped out -- and a swapped file is
a *different inode*. Key the baseline by inode and the lookup for a replaced
`/etc/passwd` misses entirely, so the verdict is "no baseline recorded" instead
of `Modified`. `VerifyStatus::Missing` becomes unreachable too, because a file
that has been deleted has no inode left to look anything up by. The path is not
the lazy key here; it is the only correct one, which is why Tripwire and AIDE
monitor paths.

**The rule that replaces it.** Ask: *should this data survive the file at this
path being replaced by a different file?*

| answer | key | examples |
|---|---|---|
| yes -- the *location* is the subject | path | integrity baselines; `cap::file_tags`, whose `effective_tags` walks ancestors so a file's tags depend on where it lives; overlay whiteouts; records of deleted names |
| no -- the *file* is the subject | identity (`FileId { fs_id, ino }`) | ACLs (POSIX keeps them in the inode's xattrs), advisory locks, seals, record locks, immutable flags, comments, tags |

The reframing also explains the inode cases better than the old rule did. An
ACL must not transfer to a stranger's file that happens to land at the same
name -- that would be a grant nobody issued. Stated as "metadata follows the
file" that is a coincidence; stated as "this must not survive replacement" it is
the reason.

**Alternatives considered.**

| option | why not |
|---|---|
| Keep the coarse rule, fix mistakes as found | it had already produced a written plan to convert `integrity`, and the conversion would have compiled, passed its existing self-tests, and quietly disabled replacement detection. The failure is invisible at the point it is made |
| Key everything by both, and check both | doubles every lookup and makes "which one wins" a new question at each site. The disagreement between the two keys is exactly the semantic content -- collapsing it loses the answer |
| Ask the operator per table | the verdict is derivable from what each table is for, and 16 tables is too many to escalate. Only `history` is a genuine tradeoff (below) |

**What is left open.** `fs::history` (file version history) is undecided and
deliberately not converted. Inode-keyed, a rename keeps its history, which is
what Dropbox and macOS versions do. Path-keyed, you get the history of a
location, which is what a user watching one config file may expect. That is
user-visible behaviour with defensible answers on both sides, so it is not a
call to make silently.

**Where this bites:** `kernel/src/fs/{acl,vfs,sealing,reclock,immutable}.rs` are
converted; `fcomment`, `queryable`, `tags` are not yet; `integrity` and
`cap/file_tags.rs` are correct as they stand and should be left alone. Two
mechanical requirements travel with any conversion: derive the key *above* the
module lock, because `Vfs::file_identity` calls into the VFS and a module global
held across that inverts filesystem-lock -> module-state (nine such sites were
introduced and caught by `scripts/check-vfs-under-lock.py` the same day), and
add a rung that hard-links a *real* file, because the pre-existing rungs used
synthetic paths that resolve to nothing and therefore passed identically before
and after conversion.
