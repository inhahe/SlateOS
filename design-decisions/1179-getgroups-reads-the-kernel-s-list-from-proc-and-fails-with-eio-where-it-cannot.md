## 1179. `getgroups` reads the kernel's list from `/proc/self/status`, believes its last `Groups:` line, and fails with `EIO` where it cannot read it

**Date:** 2026-10-06
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a process may belong to supplementary groups -- extra groups,
beyond the one it runs as, that grant access to what is shared with them.
`setgroups` and `initgroups` have installed real ones in the kernel since
September, and the kernel checks files against them, but the C library's
`getgroups` still said "none" for every process. So `id` left out every
group a login had installed. `group_member` and the SysV IPC permission
checks said a process was outside groups the kernel had it in. The kernel
has no call that reports the list. It does print it, in the `Groups:` line
of `/proc/self/status`. `getgroups` now reads it there. Three choices had
more than one reasonable answer.

**1. Read `/proc/self/status` now, rather than wait for a native call.**

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. Read `/proc` now; ask for a call** (chosen) | `id` shows the groups a login installed, today | the kernel's own answer, today; the same path `gethostname` takes | needs a File capability and a `/proc` in the root (choice 3) |
| B. Keep "none" until the call lands | nothing, until lane A answers | no file read | a false answer about privilege, for as long as the request waits |

The call is asked for in
`requests/d-a-a-process-cannot-ask-for-its-own-supplementary-groups.md`;
when it lands, the read goes.

**2. The last `Groups:` line is believed, not the first.** The file's first
line is the task's name, written raw, and a name may hold a newline. A
program run as `x\nGroups:\t0 27` has a `Groups:` line of its own choosing
ahead of the kernel's. Nothing a process controls follows the real line, so
every `Groups:` line restarts the list and the last stands. Linux escapes
the newline; the kernel doing the same is asked for in
`requests/d-a-proc-status-writes-the-name-raw-so-a-newline-in-it-forges-lines.md`.
A malformed line before the last is forgotten; a malformed last line is no
answer.

**3. A list that cannot be read is `EIO`, not "none" and not `ENOSYS`.**
Opening any file takes a File capability, so a confined process that holds
none cannot read `/proc/self/status`.

| Option | *What changes* for a confined process | For | Against |
|---|---|---|---|
| **A. `EIO`** (chosen) | `id` says it cannot get the groups | nothing false is reported; `gethostname` fails the same way for the same reason | a program that treats any `getgroups` failure as fatal stops |
| B. "No groups" | `id` shows none | nothing stops | the false answer this change removes, kept for exactly the processes least able to check it |
| C. `ENOSYS` | `id` shows `/etc/group`'s list for the user | the call "works" | gnulib's `mgetgroups` answers from `/etc/group` on `ENOSYS`: groups the kernel may not have granted, reported as held |

`group_member` and the SysV IPC checks count no supplementary group when
the list cannot be read (the gid the process runs as still counts), as
gnulib's `group_member` answers 0 when `getgroups` fails. Tracked as
`known-issues/D-POSIX-GETGROUPS-NEEDS-A-FILE-CAPABILITY.md`.

**Also decided here, with one obvious answer each:** the copy is sorted,
as Linux's is (it sorts at `setgroups`; this kernel keeps the given
order). The SysV IPC check reads the list only when the object's group and
other bits differ, because only then can membership change the answer, and
`semop` should not pay a file read for nothing. On the host, which has no
kernel, the list is read from a stand-in `status` text with an empty
`Groups:` line, through the same reader.

**Where:** `posix/src/unistd/groups.rs`, `posix/src/unistd.rs`
(`getgroups`), `posix/src/legacy.rs` (`group_member`),
`posix/src/sysv_ipc.rs` (`granted_bits`). `services/ctest-groups` checks it
on SlateOS, a forged name included.
