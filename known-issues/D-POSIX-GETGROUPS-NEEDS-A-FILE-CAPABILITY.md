## D-POSIX-GETGROUPS-NEEDS-A-FILE-CAPABILITY — `getgroups` fails with `EIO` in a process that holds no File capability, or has no `/proc` (lane D, 2026-10-06)

**Status:** OPEN -- waiting on lane A (`requests/d-a-a-process-cannot-ask-for-its-own-supplementary-groups.md`).

**In short:** `getgroups` reports a process's supplementary groups (the
extra groups it belongs to). The kernel has no call that reports them, so
the C library reads them from `/proc/self/status`, and opening any file --
that one included -- needs a File capability. A confined process that holds
none, or one that has `chroot`ed into a tree without `/proc`, cannot read
it, and its `getgroups` fails with `EIO` rather than guessing. `id` then
says it cannot get the groups; `group_member` and the SysV IPC permission
checks count no supplementary group (the gid it runs as still counts).

**Where:** `posix/src/unistd/groups.rs` -- `supplementary_groups`;
`posix/src/unistd.rs` -- `getgroups`.

**Why not answer "no groups" instead:** that was the old answer for every
process, and it was false once `setgroups` reached the kernel: "no groups"
is a claim about the process, and nobody asked the kernel. `ENOSYS` is
wrong too: gnulib's `mgetgroups` (and `id`) take it to mean the system has
no such lists, and answer from `/etc/group` instead -- groups the kernel may
not have granted.

**The proper fix:** a native call that returns the calling process's list,
needing no capability, as `SYS_PROCESS_GET_CREDENTIALS` needs none for the
uid and gid (the request above). Then `supplementary_groups` calls it.

**Reproduce:** spawn any program that calls `getgroups(0, NULL)` with no
File capability (a self-test rung's grant of nothing): -1, `EIO`.
