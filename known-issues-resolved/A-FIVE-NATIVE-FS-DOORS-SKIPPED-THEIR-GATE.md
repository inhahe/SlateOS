### A-FIVE-NATIVE-FS-DOORS-SKIPPED-THEIR-GATE -- 2026-10-01 -- FIXED (lane A)

**In short:** five native file calls missed the check their siblings make.
Two could move another program's file position. Two worked without the
file capability every other file call asks for. One created a file nothing
ever deleted, and handed back a handle its caller could not use.

| call | missing | sibling that has it |
|---|---|---|
| `SYS_FS_SEEK_DATA`, `SYS_FS_SEEK_HOLE` | possession (`require_file_handle_owner`): any process could move any description's offset by counting handle numbers | `SYS_FS_SEEK` |
| `SYS_FS_READDIR_AT` | the File capability (READ) | `SYS_FS_LIST_DIR` |
| `SYS_FS_FALLOCATE` | the File capability (WRITE) | `SYS_FS_TRUNCATE` |
| `SYS_FS_TMPFILE` | everything. It created a *named* `.tmp_<tsc>` file nothing deleted (its doc said the file vanished at close), and never registered the handle, so the caller's own reads, writes and close were refused and the open file leaked | -- |

All five are gated now. `SYS_FS_TMPFILE` answers `NotSupported` until a
handle can hold a file that has no name (the next entry); nothing called it.
Found by listing every native handler that touches `fs::handle` or reads a
user path, beside the gates it calls. `syscall::dispatch`'s
`test_dispatch_fs_gates` checks each, as a scratch process with no
capability.
