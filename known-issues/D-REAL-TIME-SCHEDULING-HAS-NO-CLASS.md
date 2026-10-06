## D-REAL-TIME-SCHEDULING-HAS-NO-CLASS — a program cannot run real-time: `SCHED_FIFO` and `SCHED_RR` are refused, there being no class in the scheduler for them (lane D, 2026-10-06)

**Status:** OPEN -- waiting on lane A
(`requests/d-a-real-time-scheduling-has-no-class-to-run-in.md`).

**In short:** a program that must not be kept waiting -- an audio server
keeping a sound card fed, a video player -- asks to run "real-time", ahead
of every ordinary program. SlateOS's scheduler has nowhere to put it: its
32 levels all belong to `nice`, so no level is above every ordinary thread.
Until 2026-10-06 the C library answered such a request with success when
the caller had the right, and changed nothing; the program believed it was
real-time and was not. It now answers `EPERM`, as Linux answers a program
without the right, and audio servers run without real-time as they do
there. They still cannot have it.

**Where:** `posix/src/sched.rs` -- `sched_setscheduler`'s step 8 refuses;
`rt_permitted` is Linux's permission rule, kept and tested for the day the
class exists. `pthread_setschedparam` gives `sched_setscheduler`'s answer,
as glibc's does, so `EPERM` too (it said `EINVAL`, a malformed request's
answer, until later on 2026-10-06), and `pthread_create` refuses an explicit
real-time attribute with `EPERM`, as before.
`posix_spawn`'s `POSIX_SPAWN_SETSCHEDULER` asks `sched_setscheduler`'s
question once lane D's spawn attributes reach `main` (branch
`lane-d-spawn-attrs`, waiting on lane A's spawn fields) -- step 8 goes into
its `setscheduler_verdict` in that merge. The kernel's Linux ABI still
records a real-time policy in the process and says yes; that is lane A's,
in the same request.

**What is needed:** lane A's real-time band above every `nice` level, with
`SCHED_FIFO` and `SCHED_RR` semantics, and a native call to put a thread in
it and read it back. Then lane D: step 8 becomes the call, `rt_permitted`
the gate in front of it, and the getters report what the kernel holds
rather than `SCHED_OTHER` at 0 for everyone. `PTHREAD_PRIO_PROTECT` mutexes
become possible with it (design-decisions §1177).
