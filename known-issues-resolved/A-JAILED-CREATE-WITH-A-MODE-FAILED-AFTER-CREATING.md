### A-JAILED-CREATE-WITH-A-MODE-FAILED-AFTER-CREATING -- 2026-10-01 -- FIXED the same day (lane A)

**In short:** a process in a chroot jail (a container) that created a file
with any mode but 0644, or a directory with any but 0755, got an error,
and the file or directory was left there anyway. `mkstemp` asks for 0600,
so every temporary file a container made failed this way. A Linux create
used 0644 whatever it asked for until the same day
(A-LINUX-CREATE-DROPPED-MODE-AND-UMASK), which hid the fault for Linux
programs; native ones met it whenever they gave a mode.

**Where:** `fs::handle::open_resolved` and `Vfs::mkdir_mode` stamped the
mode with `Vfs::set_permissions` on the path they had already resolved.
`set_permissions` resolves again, and resolving applies the caller's jail,
so the jail was applied twice: `/jail/made` became `/jail/jail/made`, and
`NotFound`.

**Fixed:** `Vfs::set_permissions_resolved`, with `set_owner_resolved`,
`set_times_resolved`, `statvfs_resolved` and `fallocate_resolved` beside
it, for a caller holding a resolved path. Test: `test_held_files` rung 13,
in a jail of its own: a file made with mode 0600, a directory with 0700,
and a read-only volume refusing a change through a handle opened in it.

**Not fixed, the same mistake:** `Vfs::atomic_write` and
`atomic_write_preserve` resolve the path, then call path calls with the
result. Their callers are kernel tasks, which have no jail, so nothing goes
wrong today. The fix, when a jailed caller appears, is the `*_resolved`
calls.
