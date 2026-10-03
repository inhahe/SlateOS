## D-POSIX-SETPRIORITY-RENICED-THE-CALLER — `setpriority` and `getpriority` acted on the calling program whatever process they were given (lane D, 2026-09-27) — **Status: FIXED 2026-09-27 (they refuse); reaching another process waits on lane A**

**In short:** `renice -n 10 -p 1234` asks the C library to lower process
1234's priority. The library ignored the 1234, lowered `renice`'s own
priority instead, and said it had worked -- so `renice` printed that 1234 had
changed when nothing had, and a task manager wired to it would have
reprioritised itself. Reading another process's priority returned the
caller's. Now anything but the caller is refused, and `renice` says so.

| Target | Was | Now |
|---|---|---|
| `PRIO_PROCESS`, `who` 0 or the caller's pid | the caller | the caller |
| `PRIO_PROCESS`, another pid | the caller, success | `-1`: `ESRCH` if no such process (Linux's answer), `EPERM` if there is one |
| `PRIO_PGRP` / `PRIO_USER`, any `who` | the caller, success | `-1` with `EPERM` (`ESRCH` for a group with no member) |

The group and user forms are refused even for the caller's own group or user:
on Linux they act on every process in the group or of the user, so doing it to
the caller alone would be the same false success, in part.

**Where:** `posix/src/resource.rs`, `prio_target_is_caller`. The native calls
behind these, `SYS_PROCESS_GET_NICE` and `SYS_PROCESS_SET_NICE`, take no
process argument and act on the caller; the kernel's Linux-ABI
`sys_setpriority` does take `who`, but a native program cannot reach it.

**Still open:** a native call that names another process, with the rule for
who may renice whom, and `/proc/<pid>/stat`'s nice field (always 0) -- both
lane A's, in `requests/e-ad-renicing-another-process-renices-the-caller.md`.
When the call exists, `prio_target_is_caller` becomes the route to it.
