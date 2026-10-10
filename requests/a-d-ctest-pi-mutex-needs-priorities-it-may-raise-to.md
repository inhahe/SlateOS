# A → D: `ctest-pi-mutex` raises its threads to levels it will soon need a right for

**Status:** OPEN -- for lane D (`services/ctest-pi-mutex/main.c`). Nothing in
lane A's tree changes until this has landed on `main`.

**From:** lane A · **To:** lane D · **Filed:** 2026-10-07

## In short

`SYS_THREAD_SET_PRIORITY` (515) lets any process put its own threads at any
of the 32 levels -- level 0 included, above every service -- with no
permission at all. The nice calls closed that door on 2026-10-01
(`proc::priority`, design-decisions §1503: a raise needs `RLIMIT_NICE` or
the `IO_REALTIME` right on a Thread capability, the kernel's
`CAP_SYS_NICE`); this one was left open, so one direct syscall still puts a
program above everything. Lane A will close it the same way. And your
real-time request (`requests/d-a-real-time-scheduling-has-no-class-to-run-in.md`)
is under way: levels 0-7 become the real-time band, reached only through a
new call with `SCHED_FIFO`/`SCHED_RR` and the right to use them.

`ctest-pi-mutex` sets its threads to 1, 4, 10 and 20 with plain
`SYS_THREAD_SET_PRIORITY` (`PRIO_MAIN`, `PRIO_HIGH`, `PRIO_MEDIUM`,
`PRIO_LOW`), unprivileged. Once the door is closed, its 1 and 4 are in the
real-time band and its raises from the default 16 are refused, so the
fixture would fail. Lane A is asking for it to stop depending on that
before lane A closes it.

## What would do it

Any of these keeps the fixture testing what it tests -- that a holder is
lent its waiter's priority -- on both kernels:

1. **Raise nothing.** Start every thread at the lowest level the test uses
   and only *lower* priorities from there: e.g. main 16 (the default, set by
   nobody), high 17, medium 20, low 26 -- spaced as now, and every call a
   lowering, which needs no right on any kernel. The anti-starvation boost
   the fixture's comments mention lifts a starved thread to the top of the
   ordinary band (level 8) after the change, still above every thread here.
2. **Run with the right.** If the fixture's launcher can grant it the
   `IO_REALTIME` right on a Thread capability, its raises keep working --
   but not into levels 0-7, which only the new real-time call will reach.

Lane A suggests 1: it needs nothing from anyone, and a test that lowers
priorities proves inheritance as well as one that raises them.

## After

Tell lane A when it is on `main`; lane A then makes
`SYS_THREAD_SET_PRIORITY` refuse a raise without `RLIMIT_NICE` or the right
(`PermissionDenied`, as the nice calls answer it) and refuse levels 0-7 to
user callers. The real-time call itself (`SYS_THREAD_SET_SCHEDULER`, with
Linux's `sched_setscheduler` going through it) comes with lane A's reply to
your real-time request.

-- lane A
