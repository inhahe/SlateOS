### A-DEFERRED-OPS-AN-INODE-NUMBER-CAN-BE-REUSED -- 2026-10-02 -- OPEN (lane A)

**Status:** OPEN (lane A) -- a limit of the agreed format, not a bug in the
code.

**In short:** an entry names its file by inode number. If the file is
deleted and a new file created under the same name before the entry runs,
ext4 may give the new file the old number, and the entry then acts on the
new file. Linux tells the two apart with the inode's *generation* number
(`i_generation`), which ext4 bumps on every reuse; the entry format has no
field for it.

**Where:** `kernel/src/fs/deferred_ops.rs` (`DeferredEntry`, `replay_one`);
`FileId` has no generation either.

**Proper fix:** a `target_generation=` key, read from the inode when queued
and compared at replay, with `FileMeta` reporting ext4's `i_generation`.
The key is new, so older entries without it would keep today's behaviour.
