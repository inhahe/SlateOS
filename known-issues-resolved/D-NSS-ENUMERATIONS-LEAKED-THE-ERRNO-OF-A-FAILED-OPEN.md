## D-NSS-ENUMERATIONS-LEAKED-THE-ERRNO-OF-A-FAILED-OPEN — `getpwent`, `getservent`, `getaliasent` and the other enumerations set `errno` when their file was missing, where glibc keeps the caller's (lane D, found 2026-10-06)

**Status:** FIXED 2026-10-06 (posix/src/nss_files.rs, posix/src/netdb.rs, posix/src/aliases.rs)

**In short:** the `get*ent` functions walk a system file (`/etc/passwd`,
`/etc/services`, `/etc/aliases` and the like) one entry at a time. When the
file is missing or unreadable, glibc's walk just ends, and `errno` is left
as the caller set it: glibc saves `errno` around the open it makes. Ours
said in its documentation that it did the same, but on SlateOS it left the
failed open's `ENOENT` or `EACCES` in `errno`. A program that clears
`errno`, enumerates, and checks `errno` to tell the end of the walk from a
failure saw a failure that never happened. The same went for an
`/etc/aliases` `:include:` whose file is missing, which glibc skips
without a trace.

The host tests could not see it. They feed each file through a hook, and
the hook, unlike the real `open`, never touched `errno`. So every
enumeration test passed on the host while the target behaved otherwise.

**Where:** `posix/src/nss_files.rs` (`Cursor::open`, the shared
enumeration cursor), `posix/src/netdb.rs` (its own cursor, for services,
protocols, networks and RPC), `posix/src/aliases.rs` (`read_include`).

**Fix:** each saves `errno` before the read and puts it back after, as
glibc's `_nss_files_getXXent_r` does around its `internal_setent`. The test
hooks now set `errno` exactly as a failed `open` would: `ENOENT` for a file
the test gave none, the given error for an unreadable one. A caller that
leaks an open's `errno` now fails on the host as it would on the target.
That is how this was found: the oracle replays of aliases and of the RPC
database failed at once, on the lines where glibc's answer says
`errno=kept`.

**How it is held:** `aliases.rs`'s
`every_answer_is_glibcs_where_glibc_agrees_with_itself` and
`an_unreadable_file_is_its_error`, and `netdb.rs`'s
`every_rpc_answer_is_glibcs` and `an_unreadable_file_is_its_error_not_a_miss`,
replay glibc's `errno` for each call, against hooks that now behave as the
target's files do.
