## D-POSIX-TRYJOIN-CANNOT-SEE-A-KILLED-THREAD — `pthread_tryjoin_np` and `pthread_timedjoin_np` never see a thread killed before it could exit (lane D, 2026-09-28) — **Status: OPEN -- both halves are written: lane A's `SYS_THREAD_JOIN_TIMEOUT` (1085, on `lane-a`) and lane D's joins on it (2026-10-07). It closes when lane A's kernel is on `main` and `services/ctest-timed-join` passes in a lane D boot.**

**In short:** a program can ask "has that thread finished?" without waiting
(`pthread_tryjoin_np`), or wait only so long (`pthread_timedjoin_np`). Both
work for a thread that ends normally. A thread that is killed instead -- by
a fault it did not handle -- never gets to say it has ended, so both keep
answering "still running" (`EBUSY`, then `ETIMEDOUT`), while a plain
`pthread_join` would have returned at once with `PTHREAD_CANCELED`.

**Where:** `posix/src/pthread.rs`, `join_readiness`: "exited" is the
thread's slot's `STATE_EXITED`, which the thread sets itself in
`pthread_exit`. The kernel knows better -- `SYS_THREAD_JOIN` reports a
killed thread as `Cancelled` -- but it only answers once the thread is gone,
and waits until then.

**Why it is still open:** the one call that can tell "gone" from "running"
without the thread's help is the kernel's, and it has no form that returns
at once or gives up at a deadline.

**Lane D's half, done 2026-10-07:** `posix/src/pthread.rs`, `kernel_join`:
`pthread_tryjoin_np` asks the kernel's join without waiting when the thread
has not said it exited, and `pthread_timedjoin_np` and
`pthread_clockjoin_np` hand it the time left on their clock, measured again
after a wait a signal ends -- one call instead of a look every millisecond.
A killed thread then joins at once as `PTHREAD_CANCELED`, as
`pthread_join`'s does; a second joiner is `EINVAL`, glibc's answer; a thread
already gone is joined whatever the deadline. On a kernel without the call
the old loop runs. `services/ctest-timed-join` checks the answers every
program sees (`EBUSY`, `ETIMEDOUT` on time, a join when the thread exits,
`pthread_clockjoin_np`) and passes on Linux; a killed thread is SlateOS's
own case, which a fixture cannot also run on Linux, where an unhandled fault
ends the whole process.

**What was asked of lane A:** lane A's join with a deadline (the request proposes a
timeout argument to `SYS_THREAD_JOIN`, 0 meaning "don't wait"); then
`pthread_tryjoin_np` asks it with 0, and `pthread_timedjoin_np` with the
time left, instead of polling the slot every millisecond as it does now.
