### A-CHATTR-MARKS-WERE-HONOURED-PIECEMEAL -- 2026-10-02 -- FIXED (lane A)

**Status:** FIXED on lane-a-wip 2026-10-02, awaiting a boot (design-decisions
§1524).

**In short:** a file marked immutable could still be replaced by renaming
another file onto it, re-moded, re-owned and hard-linked; an append-only file
could be deleted and renamed, and on ext4 overwritten whole; a file in an
immutable ext4 directory could be created or deleted; on FAT, whose read-only
bit was reported as the immutable mark, nothing was refused at all, and
`chattr +a` on a FAT file succeeded and did nothing. What was refused said
"Permission denied" (`EACCES`), where Linux -- and `rm`, `mv` and `chattr`
quoting it -- says "Operation not permitted" (`EPERM`). `access(W_OK)` on an
immutable file answered yes, and opening one for writing succeeded until the
first write.

**Where it was:** each filesystem checked a different subset on its own:
memfs most, ext4 the write path, unlink and unnamed files, FAT none, and none
of them `rename`, `link`, `chmod`, `chown` or `utimes`.

**Fix:** `fs::attr_policy` holds Linux's rules once; the VFS applies the
name and metadata rules on every filesystem under its lock (every path,
pinned-directory and held-file entry point), the filesystems the content
rules on the inode they write, the handle layer `may_open` and `F_SETFL`, and
the Linux `access` the `W_OK` rule. Every refusal is `NotPermitted`.
Verified by `fs::attr_policy::self_test` (the rules, 41 cases),
`fs::vfs::self_test_attr_rules` (50 operations through the VFS on `/tmp`,
each refused one checked unchanged after, each allowed once cleared),
`fat::format_self_test` (a read-only FAT file), the ext4 and memfs self-tests
(now expecting `EPERM`), and the ring-3 `Linux file flags` test
(`chattr`'s ioctls as root and as the file's owner).
