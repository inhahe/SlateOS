## 1525. The `chattr` marks have one home, the inode's `FileAttr`: the decorative `fs::immutable` store is removed

**Date:** 2026-10-02 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** the kernel had two separate records of "this file is
immutable / append-only". One, `vfs::FileAttr`, lives with the file (ext4's
inode flags, memfs's node, FAT's read-only bit) and is enforced everywhere
since §1524. The other, `fs::immutable`, was a table of paths nobody checked:
a file marked there could be written, deleted and renamed. Its only users
were the kshell command `fflags` and `/proc/immutable`. The table, the
command and the `/proc` file are gone; `chattr` and `lsattr` (kshell and
lane B's) are the way to set and see the marks.

| | What changes | For | Against |
|---|---|---|---|
| **Keep `FileAttr`, remove `fs::immutable` (chosen)** | `fflags` and `/proc/immutable` disappear; nothing that was protected stops being protected, because nothing was | one meaning, enforced; the marks persist with the file (ext4) and end with it, as Linux keeps them in the inode | its extra marks -- no-delete, no-backup, no-index, compressed -- go with it; none was enforced or read by anything |
| Keep both | nothing removed | -- | a mark set with `fflags` reported success and protected nothing: worse than no command, since it gives a reason to rely on it |
| Make `fs::immutable` the enforced one | -- | -- | a path-keyed table beside the filesystems, forgotten at reboot, which every filesystem's own flag would contradict |

The extra marks are not lost as ideas: no-delete has no Linux equivalent and
nothing asked for it; no-backup and no-index belong to the backup program and
the indexer if either ever wants them (Linux has `chattr +d`, `FS_NODUMP_FL`,
for the first), as `FileAttr` bits with a filesystem that can store them.

Known-issues `[A] Two implementations of file immutability` is closed by
this; its 2026-09-18 note left the choice open because neither side was
complete -- the enforced side now is.
