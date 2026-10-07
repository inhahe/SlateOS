## D-POSIX-SETPRIORITY-RENICED-THE-CALLER — `setpriority` and `getpriority` acted on the calling program whatever process they were given (lane D, 2026-09-27) — **Status: FIXED 2026-09-27 (they refuse); reaching another process: both halves are written -- lane A's `SYS_PROCESS_GET_PRIORITY`/`SET_PRIORITY` (1088/1089, on `lane-a`) and lane D's route to them (2026-10-07). It closes when lane A's kernel is on `main` and `services/ctest-renice` passes in a lane D boot.**

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

**Lane A's half** (on `lane-a`): `SYS_PROCESS_GET_PRIORITY` and
`SYS_PROCESS_SET_PRIORITY` take Linux's `which` and `who`, and the kernel
decides who may (`kernel/src/proc/priority.rs`, design-decisions §1503): the
caller, its child, or a holder of a `Process` capability with `DELETE` rights
for the target; a raise only within the target's `RLIMIT_NICE` or with a
Thread capability carrying IO_REALTIME. `SYS_PROCESS_SET_NICE` checks the
raise itself now, too. `/proc/<pid>/stat`'s nice field is still lane A's, in
`requests/e-ad-renicing-another-process-renices-the-caller.md`.

**Lane D's half, done 2026-10-07:** `posix/src/resource.rs`,
`native_priority`. `getpriority` and `setpriority` ask the kernel's calls
for any `which` and `who`, and map its answers to Linux's errnos -- no
authority `EPERM`, a refused raise `EACCES`, no such process `ESRCH`, a bad
`which` `EINVAL`. The kernel decides a raise, so the library no longer
second-guesses it with its own projection of `CAP_SYS_NICE`. `nice()` is
glibc's and musl's: `setpriority` on the caller, with `EACCES` reported as
`EPERM`. A kernel without the calls gets the behaviour above, unchanged.
Two smaller fixes came with it:

- `getpriority` set `errno` to 0 on success. POSIX forbids that ("no
  function in this volume shall set errno to 0"), and glibc does not; a
  caller clears it first, which is what every correct caller already does.
- `setpriority` and `nice` ignored `SYS_PROCESS_SET_NICE`'s answer, so on
  lane A's kernel, which refuses a raise beyond `RLIMIT_NICE` there too, a
  refused change was reported as made. The answer is checked now.

`services/ctest-renice` checks it: a parent lowers its child, a raise is
`EACCES`, pid 1 is `EPERM`, no such process is `ESRCH`, a group reads as its
most favoured member, and `nice(-1)` is `EPERM`. It passes on Linux as an
ordinary user.
