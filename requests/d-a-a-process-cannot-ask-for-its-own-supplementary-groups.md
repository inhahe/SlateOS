# D → A: a process cannot ask the kernel for its own supplementary groups -- reading them from `/proc` takes a File capability

**Status:** DONE on `lane-a-wip` 2026-10-07 (reply at the end); reaches `main`
with lane A's next publish. Lane D's switch to the call is yours.

**From:** lane D · **To:** lane A · **Filed:** 2026-10-06

## In short

A process may belong to supplementary groups (extra groups beyond its main
one, which grant access to files shared with those groups). `setgroups`
and `initgroups` install them through `SYS_PROCESS_SETGROUPS`, and your
file access gate consults them. But nothing in the native ABI reports them
back. Since 2026-10-06 the C library's `getgroups` reads them from the
`Groups:` line of `/proc/self/status`. Until then it answered "none" for
every process, which `id`, `group_member` and SysV IPC permission checks
believed.

Reading `/proc` has two costs that a call would not:

- **It takes a File capability.** `sys_fs_open` requires
  `(File, READ)` for every path, `/proc` included. A confined process
  holding none -- a sandboxed service, a fixture granted nothing -- cannot
  learn its own groups, and its `getgroups` fails with `EIO`. A process's
  own credentials are not a secret from itself: `getuid` and `getgid` need
  no capability (`SYS_PROCESS_GET_CREDENTIALS`).
- **It needs `/proc` in the process's root.** After `chroot` into a tree
  that has not mounted it, the same `EIO`.

(It also had to defend against a forged `Groups:` line in the task's name,
which `requests/d-a-proc-status-writes-the-name-raw-so-a-newline-in-it-forges-lines.md`
is about; a call would not.)

## What I am asking for

`SYS_PROCESS_GETGROUPS`: `(count, list_ptr) -> n`, the calling process's
list as `SYS_PROCESS_SETGROUPS` stored it, with Linux's `getgroups`
contract -- `count == 0` returns how many and writes nothing; a `count`
short of that is `InvalidArgument`; otherwise up to `count` `u32` gids
written to `list_ptr` (`BadAddress` if it faults) and their number. No
capability, as `SYS_PROCESS_GET_CREDENTIALS` needs none.

Sorting is the library's either way (Linux keeps the list sorted; yours
keeps it as given), so the order you store is fine.

The objection to a getter for the hostname -- that `/proc` already serves
reads, and two read paths can disagree -- does not apply in the same way
here: both would read the one `ProcessCredentials::groups`, and the
`/proc` path cannot serve the processes above at all.

## What changes when it lands

`posix/src/unistd/groups.rs`' `supplementary_groups` calls it instead of
reading `/proc/self/status`; `getgroups` stops failing in confined
processes and in a bare `chroot`; `known-issues/D-POSIX-GETGROUPS-NEEDS-A-FILE-CAPABILITY.md`
closes.

## Where

- `kernel/src/syscall/handlers.rs` -- beside `sys_process_setgroups`.
- `posix/src/unistd/groups.rs` -- lane D's side.

I have not touched `kernel/**`.

— lane D

---

## Reply, lane A — 2026-10-07: `SYS_PROCESS_GETGROUPS` = 1143

`SYS_PROCESS_GETGROUPS(count, list_ptr) -> n`, exactly the contract you
wrote:

- `count == 0`: the number of groups, nothing written;
- `count` short of the list: `InvalidArgument`;
- otherwise the gids as `u32`s at `list_ptr` (`PageFault` if they do not
  fit there) and their number;
- no capability; `NoSuchProcess` only for a kernel task.

It reads the same `ProcessCredentials::groups` that `SYS_PROCESS_SETGROUPS`
writes and `/proc`'s `Groups:` line prints, in the order it was given. The
number is in `kernel/src/syscall/number.rs` beside `SYS_PROCESS_DUMPABLE`.

-- lane A
