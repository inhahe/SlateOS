## 1170. POSIX per-process timers answer `ENOSYS` from `timer_create` until the kernel has native ones, rather than being built on the one interval timer

**Date:** 2026-10-05
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** a C program can ask for a timer that sends it a signal when
it expires (`timer_create`, then `timer_settime` to start it). SlateOS's
kernel has no such timers yet, and until now the library said yes and
started nothing, so a program waiting for one waited forever. Now
`timer_create` answers "not supported" (`ENOSYS`), which programs are
written to handle: GNU `timeout`, for one, quietly falls back to `alarm`.
The library could instead have faked these timers on top of the one alarm
clock every process has. It does not: that clock is shared with `alarm` and
`setitimer`, and a fake would break them, or itself, in ways no program
could detect.

| Option | What a program sees |
|---|---|
| **A. `timer_create` answers `ENOSYS`** (chosen) | creation fails, and the caller falls back, as on a Linux kernel built without POSIX timers |
| B. `timer_create` succeeds; `timer_settime` refuses to start it | GNU `timeout` prints "warning: timer_settime: Function not implemented" on every run, then falls back |
| C. Build them on `SYS_ITIMER_SET`, the process's one `ITIMER_REAL` | one timer works, if the program uses nothing else; a second, or `alarm`, silently cancels the first |
| D. Keep the stub | "armed"; nothing ever fires |

**Why not C.** The kernel's interval timer always delivers `SIGALRM`. A
POSIX timer may ask for any signal -- `SIGRTMIN + n` is the usual choice --
and its signal carries `si_code = SI_TIMER`, the timer's id and its overrun
count. The library could produce those only by catching `SIGALRM` in its
own trampoline and re-raising something else, which collides with the
program's own `SIGALRM` handler, with its signal mask, and with `alarm`'s
answer for the time remaining; several timers would need a queue of
deadlines kept by the library and driven from a signal handler. Each of
those is a new way to be wrong without saying so. `ENOSYS` is a way to be
unsupported out loud, which callers handle.

**Why not B.** GNU `timeout` (coreutils 9.5, `settimeout`) stays silent only
when `timer_create` fails with `ENOSYS`. It warns on any other errno, and
on any failure of `timer_settime`. A program creates a timer in order to
start it, so failing at creation loses nothing that B would keep. And a
Linux kernel built without `CONFIG_POSIX_TIMERS` answers `ENOSYS` from
`timer_create`, as SlateOS's own Linux ABI does after its argument checks,
so A makes the two ABIs agree.

**The other four** (`timer_settime`, `timer_gettime`, `timer_delete`,
`timer_getoverrun`) answer `EINVAL`: no id can name a timer. That is
Linux's answer for an unknown id, and SlateOS's Linux ABI's. A Linux kernel
without POSIX timers answers `ENOSYS` to all five instead, but there the
caller never had an id to pass.

**Revisit when** lane A lands native per-process timers
(`requests/d-a-posix-timers-and-the-dumpable-flag-need-native-calls.md`).
The five then become calls over them, and this entry is superseded.

**Where:** `posix/src/time.rs`, the "POSIX per-process timers" section;
`known-issues-resolved/B-POSIX-TIMER-SETTIME-REPORTS-SUCCESS-AND-ARMS-NOTHING.md`.
