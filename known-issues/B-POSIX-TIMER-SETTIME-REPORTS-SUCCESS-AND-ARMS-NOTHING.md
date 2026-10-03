## B-POSIX-TIMER-SETTIME-REPORTS-SUCCESS-AND-ARMS-NOTHING (lane B, 2026-10-02; the fix is lane D's)

**Status:** OPEN -- lane D's to fix; `timeout` works around it with `setitimer`.

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
