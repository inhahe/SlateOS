## B-POSIX-TIMER-SETTIME-REPORTS-SUCCESS-AND-ARMS-NOTHING (lane B, 2026-10-02; the fix is lane D's)

**Status:** FIXED 2026-10-05

**In short:** a C program on SlateOS that asks for a timer with
`timer_create` and `timer_settime` -- "send me `SIGALRM` in five seconds" --
is told the timer is set, and it never fires. A program that waits for it
waits forever. `alarm` and `setitimer`, the older calls for the same thing,
do work.

**Where.** `posix/src/time.rs`, the "POSIX per-process timers" section:
`timer_create` claims a slot in a table, `timer_settime` copies the value into
it and returns 0, and nothing reaches the kernel. The kernel's Linux ABI has
real ones (`kernel/src/syscall/linux.rs`, `sys_timer_settime`); the native
library has no route to them.

**History.** `known-issues-resolved/B-POSIX-TIMERS-SUCCEED-AND-ARM-NOTHING.md`
is the same defect for `alarm`, `ualarm` and `setitimer`, fixed on 2026-09-12
through the kernel's native interval timer (`SYS_ITIMER_SET`). These two
calls were left; this is the remainder, logged now because a port needed them.

**Who it bit.** GNU `timeout` arms its timer with exactly these calls wherever
`configure` finds them. The port in `userspace/coreutils/src/bin/timeout.rs`
uses upstream's other arm, `setitimer`, which works -- see its module docs --
so `timeout` is not affected. Any other C program using POSIX timers is, among
them, potentially, the interpreter: `build/spike/python-slateos.elf`
references all three of `timer_create`, `timer_settime` and `timer_delete`.

**The proper fix (lane D).** Arm a real timer -- one per process through
`SYS_ITIMER_SET` may already cover what callers ask for, `CLOCK_REALTIME` or
`CLOCK_MONOTONIC` with `SIGEV_SIGNAL` -- or, until that exists, fail with
`ENOSYS` so that a caller can fall back. Asked in
`requests/b-d-two-calls-gnu-timeout-makes-are-not-native-yet.md`.

**Fixed (2026-10-05, lane D):** the second of those two answers.
`timer_create` checks its arguments in Linux's order and answers `ENOSYS`;
`timer_settime`, `timer_gettime`, `timer_delete` and `timer_getoverrun`
answer `EINVAL`, there being no timer for an id to name. No call reports a
timer armed any more. GNU `timeout`'s own source falls back to `alarm`
without a word on exactly that answer, and warns on any other -- pinned by
`test_gnu_timeout_falls_back_to_alarm_without_a_warning` in
`posix/src/time.rs`. The kernel's Linux ABI turned out to have no timers
either: its `timer_create` makes these same checks and then answers
`ENOSYS`, so "real ones" above was the checks only. Timers that fire need
the kernel, and `requests/d-a-posix-timers-and-the-dumpable-flag-need-native-calls.md`
asks lane A for them; design-decisions 1170 says why the library does not
build them on the one interval timer meanwhile.
