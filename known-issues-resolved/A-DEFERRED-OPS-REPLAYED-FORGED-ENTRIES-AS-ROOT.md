### A-DEFERRED-OPS-REPLAYED-FORGED-ENTRIES-AS-ROOT -- 2026-10-02 -- FIXED (lane A)

**Status:** FIXED 2026-10-02 on `lane-a-wip` (design-decisions §1529), found
while building the system calls lane B asked for.

**In short:** the queue of deferred deletes and renames ran its entries as
whoever the entry said had queued them, trusted any file in the queue
directory, and checked only the target's inode *number* -- not which volume
it was on. Since every mount replays its queue, an ext4 USB stick carrying a
hand-written entry ("delete `/etc/shadow`, inode 1234, queued by uid 0")
would have deleted that file on the system the moment it was mounted. And
any process could write such an entry into the system volume's own queue
directly: the kernel enforces no mode bits.

**What was wrong, all in `kernel/src/fs/deferred_ops.rs`:**
- `replay_one` compared `stat(target_path).ino` with the stored number:
  the same number on another volume passed, and `stat` followed a final
  symlink that `remove` then did not.
- Every file named by a number in the queue was an entry; nothing said the
  kernel had written it.
- `queue_dir_path` pushed `/.deferred-ops` onto the mount path, and
  `PathBuf::push` replaces a path with an absolute one: every volume's queue
  was the root volume's, and every mount replayed the root's.

**Fixed by:** an entry acts only if its name, not followed, still leads to
its inode *on the volume being replayed* (`FileId` with that mount's
`fs_id`); only entries sealed immutable -- which only root can do -- are
honoured, and the kernel verifies each after sealing it; the queue path is
`<mount>/.deferred-ops` (relative name). `fs::deferred_ops::self_test` 11
and 12 hold the first two: an unsealed root entry and a sealed entry naming
a file on `/tmp` both leave their targets alone.
