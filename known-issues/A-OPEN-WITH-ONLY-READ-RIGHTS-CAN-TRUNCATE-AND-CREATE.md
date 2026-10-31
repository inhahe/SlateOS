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
asks `CREATE` for the same act. Worse, `SYS_FS_WRITE` asks no capability
at all -- only that the caller own a handle open for writing -- so a holder
of `File` `READ` -- a sandboxed reader, a service `/etc/startup.conf` gave
`File:READ` -- could open any file its identity may write for writing, and
write it. The open is the only gate on writing a file. Driver processes no
longer hold `File` (they read their firmware through `FileTree`,
design-decisions 1595, which refuses every flag but reading).

**Who sees it.** Every process given `File` `READ` without `WRITE`. Normal
user processes hold every `File` right, so it changes nothing for them.

**The proper fix.** One helper every open path calls with the flags: `READ`
always; `WRITE` too for `WRITE`, `TRUNCATE` or `APPEND`; `CREATE` too for
`CREATE` -- as `sys_fs_create` and `SYS_FS_WRITE` already ask. Then find, with
a boot test, any process given `READ` alone that opened to write (none of
the kernel's own spawns does: each such grant is for a program that only
reads), and give it what it uses.

**Fixed (lane A, 2026-10-30).** `require_open_rights` in
`kernel/src/syscall/handlers.rs`: `READ` always, and `WRITE` too for an open
for writing. Truncating, appending and creating ask nothing more, since the
VFS refuses each without the `WRITE` bit (`open_resolved_within`) -- so
`O_RDONLY | O_CREAT` of a file that exists, which `flock(1)` opens, is a
read, as on Linux. Every open path asks it: by name through
`open_authority` (`sys_fs_open`, `sys_fs_open_mode`, the kernel-path helpers
and the Linux `open`/`openat`), `sys_fs_openat2`, `SYS_FIFO_OPEN` (a write
end is writing), and the Linux reopen through `/proc/self/fd` of a named
pipe's end for one it does not hold. A create asks `WRITE`, not `CREATE`
(known-issues A-AN-OPEN-THAT-CREATES-ASKS-WRITE-NOT-CREATE). Checked by
`proc::spawn::self_test_open_rights`, from ring 3: a process holding `READ`
alone reads, `O_CREAT` of an existing file too, but neither truncates,
creates, nor opens a pipe's write end, natively or by `/proc/self/fd`, and
the file is whole after; one holding `READ` and `WRITE` does each. The first
review of the fix found it missed `SYS_FIFO_OPEN` and the reopen, and asked
`WRITE` for `O_RDONLY | O_CREAT`.
