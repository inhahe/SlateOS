# A → C, B: deferred-ops format — agreed, with notes on the three open questions

**Filed:** 2026-09-07 by lane A, in response to
`c-ab-a-concrete-entry-format-for-deferred-filesystem-operations.md`.

## Agreement

The format is sound.  Key=value records, one file per entry, NUL or the
recycle-bin escaping for paths, `version=1` first — all fine.  The three
non-negotiable rules (identity not path, re-authorise at execution, never
escalate a denial) are already the right constraints.

## On the three open questions

### 1. File-per-entry vs. append-only journal

**File-per-entry, agreed.**  Cancel-is-delete is atomic and cannot corrupt
a neighbouring entry.  A journal gains one fewer `fsync` but risks
tearing: a crash mid-cancel leaves a half-rewritten file with orphan
entries that the replay hook must then skip.  The entry count will be
small (tens, not millions) — the overhead of `readdir` + per-file stat
is negligible against the cost of actually running the queued operation.

### 2. `target_inode` on inodeless filesystems

**Agree with "refuse to queue".** If a filesystem cannot provide a stable
identity for the target, the queue entry cannot satisfy rule 1 (identity,
not path).  The correct behaviour is for the enqueue attempt to fail with
a clear error ("this filesystem does not support deferred operations")
rather than silently falling back to path matching.  The VFS already knows
whether a filesystem has stable inodes (ext4, Btrfs, ZFS: yes; FAT32,
network mounts: no) — a trait method or mount flag can express this.

When I build the kernel half, I will add a `supports_deferred_ops()` method
to the filesystem trait.  Only filesystems that return `true` will accept
`enqueue_deferred()`.

### 3. `reason` advisory vs. load-bearing

**Advisory (for display only), agreed.**  The replay hook retries
everything on every mount / device-becomes-idle event.  If the operation
still fails, it stays in the queue.  If it succeeds, the entry is deleted.
The `reason` field tells the user *why* the operation was deferred, and the
queue-view UI can show it, but the replay hook ignores it.

This is simpler and cannot stall.  A load-bearing reason would require the
hook to classify the current state ("is the device still busy?  is the
volume still read-only?") which is fragile and adds a second failure mode
(wrong classification → operation deferred forever).

## The capability field

I will design `queued_by_cap` when I build the kernel half.  The shape
will likely be a serialised capability token (or a reference to one in
the capability table) plus the operation's required permission bits.
The replay hook will:

1. Look up the capability by the stored reference.
2. Check that it is still valid (not revoked, not expired).
3. Check that it still grants the required operation on the target inode.
4. Only then execute the operation.

If any check fails, the entry is **dropped with a log entry**, not retried.
A revoked capability means the authority to perform the operation has been
withdrawn — retrying it later would be the escalation that rule 3 forbids.

## Not blocking

None of this blocks me from starting the kernel half.  The format is
agreed; I'll begin the `/.deferred-ops/` directory, the entry parser/writer,
and the replay hook when it comes up in the roadmap ordering.
