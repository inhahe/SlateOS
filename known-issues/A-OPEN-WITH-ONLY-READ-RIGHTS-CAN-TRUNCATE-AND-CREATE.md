## A-OPEN-WITH-ONLY-READ-RIGHTS-CAN-TRUNCATE-AND-CREATE (lane A, 2026-10-30) — OPEN

**Status:** OPEN -- fixed on lane-a-wip, awaiting a boot on main

**In short:** a process given only the right to *read* files can still empty
a file or make a new one, by opening it with "truncate" or "create". Writing
data afterwards does ask for the right to write; the open itself does not,
and the damage is done at the open.

**What is wrong.** Every open path checks the `File` capability for `READ`
alone, whatever the flags ask for:

| Path | Where |
|---|---|
| native `SYS_FS_OPEN` | `sys_fs_open` (`kernel/src/syscall/handlers.rs`) |
| native `SYS_FS_OPEN_MODE` | `sys_fs_open_mode` |
| native `openat2` | `sys_fs_openat2` |
| Linux `open`/`openat` | `kernel/src/syscall/linux.rs` (asks `WRITE` for `O_TMPFILE` only) |

`OpenFlags::TRUNCATE` empties the file inside the open
(`fs::handle::open_impl`), and `CREATE` makes it, while `sys_fs_create`
asks `CREATE` for the same act and `SYS_FS_WRITE` asks `WRITE` for the
bytes. So a holder of `File` `READ` -- a sandboxed reader, a service
`/etc/startup.conf` gave `File:READ` -- can truncate any file its identity
may open for writing, and create files where it may, though it could write
nothing into them. Driver processes no longer hold `File` (they read their
firmware through `FileTree`, design-decisions 1595, which refuses every
flag but reading).

**Who sees it.** Every process given `File` `READ` without `WRITE`. Normal
user processes hold every `File` right, so it changes nothing for them.

**The proper fix.** One helper every open path calls with the flags: `READ`
always; `WRITE` too for `WRITE`, `TRUNCATE` or `APPEND`; `CREATE` too for
`CREATE` -- as `sys_fs_create` and `SYS_FS_WRITE` already ask. Then find, with
a boot test, any process given `READ` alone that opened to write (none of
the kernel's own spawns does: each such grant is for a program that only
reads), and give it what it uses.

**Fixed (lane A, 2026-10-30).** `require_open_rights` in
`kernel/src/syscall/handlers.rs`: `READ` always, `WRITE` too for `WRITE`,
`TRUNCATE` or `APPEND`, `CREATE` too for `CREATE` -- asked by every open
path above (`sys_fs_open`, `sys_fs_open_mode`, `sys_fs_openat2`, the kernel-
path helpers the Linux `open` reaches, and the Linux `openat`). Checked by
`proc::spawn::self_test_open_rights`, from ring 3: a process holding `READ`
alone neither truncates nor creates, and the file is whole after.
