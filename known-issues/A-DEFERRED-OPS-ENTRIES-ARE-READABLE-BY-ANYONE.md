### A-DEFERRED-OPS-ENTRIES-ARE-READABLE-BY-ANYONE -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A). Waits on the kernel enforcing mode bits, or on
ACLs that persist (`A-PER-FILE-STATE-OUTLIVED-ITS-FILE`).

**In short:** a queued delete or rename records the name of the file it is
for. `SYS_FS_DEFER_LIST` shows a user only their own entries, but the entry
files themselves (`<volume>/.deferred-ops/<id>`,
`/var/lib/deferred-ops/<uuid>/<id>`) can be read by any process: they are
mode 0600 and root's, and the kernel enforces neither. So anyone who looks
in the directory can see which files other users have asked to delete.
Integrity is not affected -- each entry is sealed immutable and only sealed
entries are acted on (`A-DEFERRED-OPS-REPLAYED-FORGED-ENTRIES-AS-ROOT`).

**Where:** `kernel/src/fs/deferred_ops.rs` (`write_sealed`); the missing
enforcement is the VFS's (`check_path_access` judges ACLs and tags only).

**Proper fix:** enforcing mode bits for every process, which makes the 0600
already set mean what it says. Until then a capability tag on the two
queue directories would do it, but tags are in memory and keyed by path, so
they would have to be re-applied at every mount; not done.
