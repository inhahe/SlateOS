## 1533. A file's comment is its `user.xdg.comment` attribute, not a kernel table

**Date:** 2026-10-02 · **Decided by:** Claude (operator-approved scope: §978's
doors for the per-file tables, built here to `design.txt`'s letter) · **Lane:** A

**In short:** a person can attach a comment to a file ("Q3 report, needs
review"), which the file explorer shows in its Properties dialog. The kernel
kept comments in a list of its own, in memory and keyed by the file's name:
a second name for the file (a hard link) showed none, a rename lost it, a
reboot lost them all, and no program could reach the list at all. Now a
comment is stored on the file itself, as an extended attribute (a small named
value a filesystem keeps with a file) called `user.xdg.comment`, which is
where `design.txt` says comments go ("Stored in extended attributes") and the
name Linux desktops already use. Programs read and write it with the
attribute calls they already have; that is the door §978 asked for.

| | For | Against |
|---|---|---|
| **The file's own `user.xdg.comment` attribute (chosen)** | what `design.txt` specifies; on disk on ext4, so it survives a reboot; follows the file through every name and rename and goes with it, with no bookkeeping here; the attribute calls are the door, with Linux's permission rules (`fs::xattr_policy`); KDE's Dolphin, `getfattr`, `cp -a`, `tar --xattrs` and `rsync -X` all carry it | a filesystem keeps what it can: ext4 holds a file's attributes in one block, so a comment there is limited to a few KiB, not 64; searching every comment means walking the tree, as nothing indexes them |
| The table, re-keyed by file identity, with comment system calls (as the ACLs were) | a search is a scan of one table; the 64 KiB limit holds everywhere | still in memory, so still gone at reboot; a second copy of what the filesystem can keep, kept in step by hand; new calls no other system has, where `getxattr` already works |

The ACLs' door (§1527) kept a table behind it: the permission check reads that
table on every access, and moving the ACLs into the filesystem's own attribute
is recorded as still to do (known-issues `A-PER-FILE-STATE-OUTLIVED-ITS-FILE`).
Nothing reads a comment on every access, so comments can go straight to the
attribute.

**What changed with it:**
- `fs::fcomment` is `get`, `set` and `remove` through the VFS's attribute
  calls, as the caller, plus `search` and `list`, which walk a subtree (not
  `/proc`, `/sys` or `/dev`) and say when they could not see all of it.
- `/proc/fcomment` counts operations only. It used to list every comment on
  the system to any reader, whether or not it could read the files.
- `fswalk` now counts the directories a walk leaves unread (its depth limit,
  its queue). It dropped them silently, so a search could report nothing
  found when it had not looked.
- kshell's `fcomment append` is gone: as a read and then a write it could lose
  a comment written between the two.

**Not done:** comments that existed only in the old table were in memory and
went at the next boot anyway, so there is nothing to carry over.
