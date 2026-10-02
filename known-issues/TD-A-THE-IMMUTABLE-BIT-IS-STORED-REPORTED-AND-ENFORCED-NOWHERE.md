## TD-A-THE-IMMUTABLE-BIT-IS-STORED-REPORTED-AND-ENFORCED-NOWHERE (lane A, 2026-09-14)

**In short:** a file marked immutable can be overwritten. The bit is read from
the ext4 inode, reported through `stat`, written back when set, and refused only
by `access(W_OK)` -- which a program that simply writes never calls.

| piece | where | state |
|---|---|---|
| `IMMUTABLE = 0x10` | `ext4/ondisk.rs:576` | exists |
| read into `FileAttr::IMMUTABLE` | `ext4/vfs_impl.rs:60` | exists |
| written back to `i_flags` | `ext4/vfs_impl.rs:1090` | exists |
| `Vfs::is_writable` denies on it | `vfs.rs:5174` | exists, **no production caller** |
| `access(W_OK)` denies on it | `vfs.rs:5217` | exists, and is advisory |
| `Vfs::write_file` checks it | -- | **does not** |

`Vfs::is_writable` has exactly one caller in the tree and it is a self-test
(`vfs.rs:7272`). So the only enforcement path is a question a writer is free not
to ask.

**Why this is the worst member of its family.** This week has turned up several
settings that are saved and do nothing -- a rebound shortcut that never came
back, fifteen private clipboards, a config export that assigned a `String`
nothing rendered. Those cost the user work. This one costs a **belief**: the
whole point of the immutable bit is that a user or an installer marks something
and then relies on it. A protection that reports success and does not protect is
worse than an absent one, because nothing prompts the caller to take a different
precaution.

**Found by** answering lane B's
`requests/b-a-chattr-needs-fs-ioc-getflags-or-it-should-be-deleted.md`, which
asked whether lane A would add `FS_IOC_GETFLAGS`/`FS_IOC_SETFLAGS`. The
interesting answer was not yes or no: the ioctl would have let `chattr +i` set a
flag with no effect, which is how the bug would first have been *noticed* -- by a
user whose file changed anyway.

**The fix, in order.** Enforce at the points where a write happens --
`write_file`, the open-for-write path, `truncate`, `remove` -- rather than only
in `access`. `FS_APPEND_FL` needs the same treatment with append-only semantics.
Only then the ioctl, which is small once enforcement holds.

**Do not fix this by giving `Vfs::is_writable` callers.** It takes a path and
re-does a metadata lookup; the write paths already hold the metadata they need.
Routing writes through it would add a second lookup per write and a TOCTOU
window between the check and the write.
