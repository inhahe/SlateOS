# A → D: a process has real, effective, saved and filesystem ids now -- the setuid family needs two native calls

**Filed:** 2026-10-08 by lane A. **Addressed to:** lane D (the C library).
**Status:** OPEN -- the kernel half is on lane-a-wip, awaiting a boot and
`main`.

## In short

The kernel keeps four user ids and four group ids per process now, as Linux
does: real, effective, saved and filesystem (design-decisions 1552). Root can
set root aside and take it back (`seteuid(1000)` … `seteuid(0)`), and gives
it up for good only when no id is 0. Linux programs have this already, through
the Linux calls. The native C library implements `setuid` and its family
itself, on top of `SYS_PROCESS_SET_CREDENTIALS`, which knows one uid. With
this change, that call sets all four ids. So a native `seteuid(1000)` is still
a permanent drop, as before, and `getresuid` cannot report a saved id. Two new
calls do it properly.

## The calls (`kernel/src/syscall/number.rs` has the full contracts)

1. **`SYS_PROCESS_SET_IDS` (1159)** `(op, a, b, c)`. `0xFFFF_FFFF` leaves an
   id as it is.

   | op | the call | ids |
   |---|---|---|
   | 0 `SET_IDS_UID` | `setuid` | `a` |
   | 1 `SET_IDS_REUID` | `setreuid` | real `a`, effective `b` |
   | 2 `SET_IDS_RESUID` | `setresuid` | real `a`, effective `b`, saved `c` |
   | 3 `SET_IDS_FSUID` | `setfsuid` | `a` |
   | 4..=7 | `setgid`, `setregid`, `setresgid`, `setfsgid` | the same |

   The kernel applies Linux's rules (`kernel/src/proc/setid.rs`), the
   permission check included, so the library's `uid_change_permitted` table
   is no longer needed for these. Answers 0, or `NotPermitted` (`EPERM`);
   `InvalidArgument` for `SET_IDS_UID` of -1. `SET_IDS_FSUID` answers the old
   filesystem id and never fails, as `setfsuid` does. Privilege is the
   `SET_CREDENTIALS` right only, not an id of 0: no authority comes from an id
   for a native program.
2. **`SYS_PROCESS_GET_IDS` (1160)** `(buf)` fills eight `u32`s: the real,
   effective, saved and filesystem user ids, then the group ids. For
   `getuid`, `geteuid`, `getresuid`, `getgid`, `getegid`, `getresgid`, and
   `setfsuid`'s old value.

`seteuid(e)` is `SET_IDS_RESUID(-1, e, -1)`, as glibc's is. `setegid` is the
same, with group ids.

## What to change in `posix/`

- `unistd.rs`: `setuid`, `seteuid`, `setreuid`, `setresuid` and the gid
  twins call `SYS_PROCESS_SET_IDS`; `getuid`/`geteuid`/`getresuid` and the gid
  twins read `SYS_PROCESS_GET_IDS`.
- `sys_fsuid.rs`: `setfsuid`/`setfsgid` call ops 3 and 7.
- On a kernel without the calls (`ENOSYS`), fall back to today's
  `SYS_PROCESS_SET_CREDENTIALS` path.

`SYS_PROCESS_SET_CREDENTIALS` stays. It is now a privileged `setuid` of every
id, for a program giving up or taking on an identity (login, `su`).

## Also changed, which the library may notice

- **A spawned child is its parent's user.** Until now every spawned child ran
  as root, `posix_spawn`'s included (`known-issues/A-SPAWNED-CHILD-RUNS-AS-ROOT.md`).
  Now it has its parent's ids, as a fork and an exec leave them. Nothing to
  change in the library, but a test that expected a root child from a non-root
  parent would now fail.
- **File access goes by the filesystem ids**, which follow the effective ids
  unless `setfsuid` set them apart.

## Fixture

`build/setidtest.c` (embedded in `kernel/src/proc/elf.rs`'s
`build_linux_setid_test_elf`) is the Linux-ABI test. Linux 6.6.87 passes it as
root twelve runs of twelve. A native twin in `services/ctest-*` would pin the
library's half.
